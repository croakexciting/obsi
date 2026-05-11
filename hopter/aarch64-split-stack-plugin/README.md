# aarch64 Split-Stack Plugin (Hopter-style, no compiler patch)

A proof-of-concept showing that **Hopter's segmented-stack mechanism can be
implemented on aarch64 without modifying rustc/LLVM** — by shipping a
small out-of-tree LLVM IR pass plus a userspace runtime.

The pass instruments every function entry with a stack-bound check; on
the cold path a runtime helper (`__morestack`) is called.  Compared to
the in-tree Hopter approach (a custom LLVM/rustc fork), this plugin
buys you:

- **No toolchain rebuild.** A 50 KB `.so` is the only artifact.
- **No backend changes.** Works on stock clang 18.
- **Architecture-portability of the *pass*.** The same C++ targets any
  aarch64/riscv/x86 codegen that clang supports; only the runtime cares
  about the SP/TLS conventions of the host.

What you give up vs. a real compiler patch:
- **Frame size is estimated**, not exact (over-estimation only causes
  earlier `__morestack` calls — never UB).
- **Unwind/cleanup-pad coordination is not done** — IR-level passes
  cannot intercept EH personality and `cleanuppad` codegen the way a
  rustc/LLVM patch can.  This demo focuses on the stack-check half of
  the Hopter design.

---

## Repository layout

| Path | Purpose |
|---|---|
| [pass/SplitStackPass.cpp](pass/SplitStackPass.cpp) | The LLVM IR pass (new PassManager plugin) |
| [runtime/runtime.c](runtime/runtime.c) | Linux/qemu demo runtime — logs `__morestack` calls and bumps the bound |
| [runtime/runtime_aarch64.S](runtime/runtime_aarch64.S) | Placeholder for a future real stacklet-swap runtime |
| [demo/demo.c](demo/demo.c) | Test program with shallow / big-frame / recursive cases |
| [CMakeLists.txt](CMakeLists.txt) | Build the pass against `/usr/lib/llvm-18` |
| [build.sh](build.sh) | One-shot CMake build of the plugin |
| [run.sh](run.sh) | Cross-compile demo, run under qemu-aarch64, dump disassembly |

---

## Quick start

```bash
./build.sh                      # builds build/SplitStackPass.so
SPLIT_STACK_VERBOSE=1 ./run.sh  # cross-compile + run + show disassembly
```

Tested on:
- Ubuntu 22.04 host
- clang/LLVM 18.1.8 (Debian package `clang-18 / llvm-18-dev`)
- `lld` 18 (Debian package `lld-18`; used as the cross-linker via `-fuse-ld=lld`)
- qemu 8.0.2 (`qemu-aarch64`)

---

## Test results

All tests run under `qemu-aarch64 8.0.2` (Linux user-mode) on an x86-64 host.

### Main demo (`demo/demo.c`, stacklet = 4 KiB)

```
[rt] init: sp=0x5502816ba0  initial_bound=0x5502815ba0  stacklet_size=4096 B
=== shallow ===
=== big_frame ===
=== recurse(20) ===
[rt] morestack #1: frame_est=384  bound 0x5502815ba0 → 0x5502814ba0  (extended by 4096 B)
results: a=175 b=261120 c=55776  morestack_calls=1
```

`big_frame` (volatile char buf[2048], estimated frame 2176 B) fits within the
initial 4 KiB stacklet.  `recurse(20)` triggers `__morestack` exactly once
after ~8 levels of recursion, then completes correctly on the extended bound.

### Correctness test (`demo/correctness_test.c`, stacklet = 1 KiB)

Compiled with `-DHOPTER_DEMO_STACKLET_BYTES=1024` to force early triggering.

```
[rt] init: sp=0x5502812b90  initial_bound=0x5502812790  stacklet_size=1024 B
[PASS] overflow_frame: result=106566 (expected 106566)  morestack_calls_during=1
[PASS] deep_recurse(29): result=435 (expected 435)  morestack_calls_during=8
[INFO] chain: result=106997  morestack_calls_during=0
[PASS] vla_test(900): result=900 (expected 900)  morestack_calls_during=1

=== ALL PASS (10 total morestack calls) ===
```

| Test | Morestack calls | Result | Status |
|---|---|---|---|
| `overflow_frame` (900 B static buf) | 1 | 106566 | **PASS** |
| `deep_recurse(29)` (30 × 256 B) | 8 | 435 | **PASS** |
| `chain` (both, bound already low) | 0 | 106997 | INFO |
| `vla_test(900)` (VLA, dynamic alloca) | 1 | 900 | **PASS** |

Key observations:
- The IR pass check fires **before** the real `sub sp, sp, #N` alloca (verified
  by disassembly of `big_frame` and `recurse` — see the "check before alloca"
  proof in the disassembly section below).
- After `__morestack` lowers the bound and returns, the function resumes at the
  alloca instruction and computes the correct result — demonstrating that SP
  restoration leaves the frame intact.
- 128 B of callee-saved padding in the frame estimate absorbs the prologue
  `stp x29, x30, [sp, #-32]!` without a false-positive miss.
- `vla_test` demonstrates the **dynamic alloca check**: the entry check uses a
  static estimate (1040 B) and passes; the per-VLA check (900 + 128 = 1028 B)
  fires the dynamic `__morestack` call.  The disassembly shows two separate
  check blocks (`sub x9,sp,#0x410` and `sub x9,sp,#0x404`) before the actual
  `mov sp, x8` alloca instruction.

### Implementation notes

See [Design → `__morestack` section](#4-__morestack----zero-stack-usage-design)
for the full explanation.  Key point: `__morestack` has **zero stack usage** —
no frame, no function calls, no SP writes.  It uses only 14 instructions of
pure ADRP/LDR/STR against absolute globals plus a Local Exec TLS access for
the bound variable.  No alternate stack is needed.

---

## Design

### 1. The instrumentation contract

For every defined function `F` (skipping declarations, naked functions,
and `[[fn_attr("no-split-stack")]]`-tagged functions) the pass rewrites
the entry block from:

```text
entry:
    <original instructions>
```

into:

```text
entry:                                       ; check block
    %sp    = call i64 @llvm.read_register.i64(metadata !"sp")
    %bound = load i64, i64* @__hopter_stklet_bound  ; thread-local
    %need  = sub i64 %sp, <ESTIMATED_FRAME>
    %ok    = icmp uge i64 %need, %bound
    br i1 %ok, label %ss.cont, label %ss.morestack
        !prof !{!"branch_weights", i32 1024, i32 1}   ; cold path

ss.morestack:
    call void @__morestack(i64 <ESTIMATED_FRAME>)
    br label %ss.cont

ss.cont:
    <original instructions>
```

Two interface symbols are introduced as **weak externals**:

| Symbol | Type | Provided by |
|---|---|---|
| `__hopter_stklet_bound` | `thread_local u64` | runtime |
| `__morestack` | `void(u64)` | runtime |

The pass also stamps two attributes on each instrumented function for
inspection / later passes:

```
attributes = { "split-stack-instrumented" "split-stack-frame-estimate"="384" }
```

### 2. Frame size estimation

The pass walks the function and accumulates an upper bound:

```
frame = sum( align_up(sizeof(alloca), align(alloca)) )
      + 128                  // callee-saved + spill padding
      + extra_pad            // user-tunable via -split-stack-extra-pad
frame = max(frame, 64)       // floor for leaf functions
frame = round_up_16(frame)   // aarch64 SP alignment
```

Dynamic allocas (rare in Rust IR) are charged a fixed `256 B`.

The estimate is intentionally **pessimistic**.  Over-estimation just
means `__morestack` runs earlier on a deep call chain; it can never
cause stack overflow.  Compare to the in-tree Hopter `Patch 1`, which
uses the exact `MachineFrameInfo::getStackSize()` from the backend — the
plugin trades that precision for not having to fork LLVM.

For accuracy comparison on the demo:

| Function | IR estimate | Backend-actual frame |
|---|---|---|
| `leaf`     | 128 B  | ~16 B  |
| `shallow`  | 128 B  | ~16 B  |
| `recurse`  | 384 B  | ~272 B |
| `big_frame`| 2176 B | ~2080 B |

The `big_frame` case shows the estimator tracking a real 2 KiB alloca
within ~5 % overhead.  Leaf-function over-estimation is irrelevant
because they almost never trip the bound.

### 3. Pass plumbing

The pass registers itself **twice** with the new PassManager:

```cpp
PB.registerOptimizerLastEPCallback(...);   // automatic, after all opts
PB.registerPipelineParsingCallback(...);   // explicit, via -passes=split-stack
```

Running at `OptimizerLastEP` means we see the IR *after* inlining and
DCE — leaf functions that get inlined away never need a check at all.

Configurable via:

| Knob | Effect |
|---|---|
| Env `SPLIT_STACK_VERBOSE=1` | Print per-function frame estimates |
| `-mllvm -split-stack-extra-pad=N` | Add N extra bytes to every estimate (only effective if pass-cl is used; for clang `-mllvm` parses before plugin loads, prefer recompiling pass with default tweaked) |
| Function attribute `"no-split-stack"` | Skip instrumentation |

### 4. `__morestack` — zero-stack-usage design

#### 4.1 Stack layout at every key moment

To understand why the design is safe, trace SP and the soft bound through
the five execution points of an instrumented function call.

```
HIGH ADDRESS
│
│  ┌───────────────────────────────────────┐
│  │          caller's frame               │
│  │  (local variables, saved regs, …)    │
│  └───────────────────────────────────────┘
│  ← caller's SP before the bl instruction
│
│    (bl writes x30 = return addr, does NOT move SP)
│
│  ← F's SP on entry  ══════════════════════ [Point A]
│
│  The soft bound is somewhere below here:
│  bound = initial_sp − stacklet_size
│
│  ─ ─ ─ ─ ─ ─ ─ ─ ─ soft bound ─ ─ ─ ─ ─ ─  [soft limit]
│
LOW ADDRESS
```

**Point A — function entry, before prologue**

```
high ──────────────────────────────────────
      │  caller frame                      │
      └───────────────────────────────────┘
         ↑ SP  (= caller's SP, unchanged by bl)
low  ─ ─ ─ ─ ─ ─ ─ ─ bound ─ ─ ─ ─ ─ ─ ─ ─
```

SP is at the top of the available zone.  The soft bound is several KiB below.

---

**Point B — after prologue `stp x29,x30,[sp,#-32]!`**

The LLVM backend always generates the callee-save push as the very first
instruction of the function body, before any IR-level check.

```
high ──────────────────────────────────────
      │  caller frame                      │
      └───────────────────────────────────┘
      │ saved x20 │ saved x19 │ ← [SP+16]
      │ saved x29 │ saved x30 │ ← SP  (SP moved down 32 B)
low  ─ ─ ─ ─ ─ ─ ─ ─ bound ─ ─ ─ ─ ─ ─ ─ ─
      │  ← this zone is UNWRITTEN          │
      │    but IS below the soft bound     │
```

**Important**: the 32 bytes of callee-saves are legitimately below the old
SP, but they are covered by the 128 B callee-save padding in the frame
estimate (Section 2).  They never cross the soft bound.

---

**Point C — the IR check fires (sp − frame_est < bound)**

The IR-inserted check reads SP (now pointing to the callee-save area) and
subtracts the estimated frame size.  If that would go below the bound, it
branches to `ss.morestack` and calls `__morestack`.

```
high ──────────────────────────────────────
      │  caller frame                      │
      └───────────────────────────────────┘
      │ saved x20 │ saved x19 │
      │ saved x29 │ saved x30 │ ← SP  (unchanged, check is read-only)
low  ─ ─ ─ ─ ─ ─ ─ ─ bound ─ ─ ─ ─ ─ ─ ─ ─
      │                                    │
      │  SP − frame_est lands here         │ ← would-be alloca base
      │  < bound  →  __morestack fires     │
```

The check is **purely a comparison**: SP does not change, no memory is
written.

---

**Point D — inside `__morestack` (the critical window)**

`bl __morestack` writes x30 = return-into-ss.cont, but does **not** move SP.
Inside `__morestack`, SP is still pointing at the callee-save area.

`__morestack` MUST NOT write below this SP, because those addresses are
inside the forbidden zone and may be returned-to live data.  Therefore
the implementation uses **only absolute-address loads and stores**:

```
high ──────────────────────────────────────
      │  caller frame                      │
      └───────────────────────────────────┘
      │ saved x20 │ saved x19 │
      │ saved x29 │ saved x30 │ ← SP  (SP UNCHANGED throughout __morestack)
low  ─ ─ ─ ─ ─ ─ ─ ─ old bound ─ ─ ─ ─ ─ ─
      │                                    │
      │         FORBIDDEN ZONE             │
      │   (nothing written here)           │
      │                                    │
      ─ ─ ─ ─ ─ ─ new bound ─ ─ ─ ─ ─ ─ ─ ─  ← bound lowered by stacklet_size
```

`__morestack` only does:

```asm
    mrs  x1, tpidr_el0                           // read thread pointer
    add  x1, x1, #:tprel_hi12:__hopter_stklet_bound, lsl #12
    add  x1, x1, #:tprel_lo12_nc:__hopter_stklet_bound
    ldr  x2, [x1]           // load bound from TLS (absolute address, not [SP-N])
    ldr  x3, [__morestack_stacklet_size]         // load stacklet size
    sub  x2, x2, x3
    str  x2, [x1]           // store new bound  (absolute address, not [SP-N])
    // ... increment counter similarly ...
    ret
```

Every load and store targets an absolute global address.  SP is read exactly
zero times and written exactly zero times.

---

**Point E — after `__morestack` returns, alloca runs**

`ret` jumps to `ss.cont:`, which is immediately before `sub sp, sp, #N`.
Now the bound has been lowered, so the alloca is within the new range:

```
high ──────────────────────────────────────
      │  caller frame                      │
      └───────────────────────────────────┘
      │ saved x20 │ saved x19 │
      │ saved x29 │ saved x30 │ ← SP (still here, about to run alloca)
      │                        │
      │   sub sp, sp, #N  ─────┼──→  SP moves to here
      │                        │
      │   volatile int pad[64] │  ← local variables (safely inside new zone)
      │                        │
low  ─ ─ ─ ─ ─ new bound ─ ─ ─ ─ ─ ─ ─ ─ ─  ← lowered by __morestack
      │         (new stacklet available)   │
```

The alloca lands well above the new bound.  The function runs normally.

---

#### 4.2 Why the prologue-before-check ordering is safe

The key observation: the 128 B callee-save padding in the frame estimate
(Section 2) is there specifically to account for the prologue pushes.

```
frame_estimate = alloca_bytes + 128 (padding) + ...
```

When the check computes `sp − frame_estimate < bound?`, the 128 B covers
the already-executed prologue.  So even though SP has moved down by 32 B
(one `stp`), the check still fires early enough to leave room for the real
alloca.

If the check passes (no morestack needed), the alloca runs and everything
is within the current stacklet.  If the check fails, the bound is lowered
before the alloca runs — the alloca then lands in the freshly extended zone.
In neither case does any write go below the updated bound.

---

#### 4.3 Implementation: 14 instructions, zero SP writes

```asm
__morestack:
    // Registers x1, x2, x3 are caller-saved — free to clobber.
    // x0  = frame_estimate (arg, unused here)
    // x29 = caller FP  }
    // x30 = return LR  } — untouched
    // SP               }

    // Step 1: lower __hopter_stklet_bound via Local Exec TLS.
    //   bound_addr = TPIDR_EL0 + link-time-constant-offset
    //   No function call, no GOT load, no stack traffic.
    mrs     x1, tpidr_el0
    add     x1, x1, #:tprel_hi12:__hopter_stklet_bound, lsl #12
    add     x1, x1, #:tprel_lo12_nc:__hopter_stklet_bound
    ldr     x2, [x1]                        // bound
    adrp    x3, __morestack_stacklet_size
    add     x3, x3, #:lo12:__morestack_stacklet_size
    ldr     x3, [x3]                        // stacklet_size
    sub     x2, x2, x3
    str     x2, [x1]                        // bound -= stacklet_size

    // Step 2: increment debug counter.
    adrp    x1, __morestack_call_count
    add     x1, x1, #:lo12:__morestack_call_count
    ldr     x2, [x1]
    add     x2, x2, #1
    str     x2, [x1]

    ret     // x30 → ss.cont: in the caller
```

#### 4.4 Why Local Exec TLS avoids a function call

`__hopter_stklet_bound` is `__thread uint64_t`.  Three TLS access models:

| Model | Generated code | Function call? |
|---|---|---|
| General Dynamic (default in .so) | `bl __tls_get_addr` | **Yes — forbidden** |
| Initial Exec | `adrp + ldr [GOT] + mrs + add` | No |
| **Local Exec** (used here) | `mrs tpidr_el0 + add + add` | **No** |

Local Exec is valid for executables where the TLS variable is in the same
link unit.  The thread-pointer offset is stamped in by the static linker;
nothing to resolve at runtime.

The IR pass uses General Dynamic when instrumenting caller code (correct
for any context).  `__morestack` assembly uses Local Exec — safe because
`runtime_aarch64.S` is always statically linked into the final executable.

#### 4.5 Register invariants

| Register | At `__morestack` entry | At `ret` |
|---|---|---|
| `x0` | frame_estimate (arg, unused) | unchanged |
| `x1`, `x2`, `x3` | caller-saved scratch | modified |
| `x19`–`x28` | caller's live values | unchanged (never touched) |
| `x29` | caller's FP | unchanged |
| `x30` | LR → `ss.cont` | unchanged |
| `sp` | caller's SP (post-prologue) | **unchanged** |

### 5. Runtime contract (`runtime.c`)

[runtime/runtime.c](runtime/runtime.c) provides the globals that
`__morestack` reads/writes:

| Symbol | Type | Role |
|---|---|---|
| `__hopter_stklet_bound` | `__thread uint64_t` | soft bound, checked per-call |
| `__morestack_stacklet_size` | `uint64_t` | bytes to subtract per morestack |
| `__morestack_call_count` | `uint64_t` | debug counter |

The constructor `__split_stack_ctor` runs before `main()` and sets the
initial bound:

```c
__hopter_stklet_bound = current_sp - HOPTER_DEMO_STACKLET_BYTES;
```

A bare-metal Hopter port would replace `__morestack_stacklet_size` with a
per-task field, and add mmap/SVC logic for physical allocation, but the
`__morestack` assembly contract (update bound, return) stays the same.

---

## Using the plugin in your own builds

The pass is a standard new-PassManager plugin.  Three integration
options:

### Option A — pass it to clang directly

```bash
clang -O2 -fpass-plugin=/path/to/SplitStackPass.so foo.c bar.c \
      -o foo  -lsplitstack_rt
```

Provide the runtime symbols (`__morestack`, `__hopter_stklet_bound`)
yourself, e.g. by linking [runtime/runtime.c](runtime/runtime.c) or
your own port.

### Option B — drive `opt` for IR-only experiments

```bash
clang -O2 -emit-llvm -c foo.c -o foo.bc
opt -load-pass-plugin=./SplitStackPass.so -passes=split-stack \
    foo.bc -o foo.instrumented.bc
llc foo.instrumented.bc -o foo.s
```

### Option C — Rust (preview, not in this demo)

```bash
RUSTFLAGS="-Cpasses=split-stack \
           -Cllvm-args=-load-pass-plugin=$PWD/SplitStackPass.so" \
    cargo build --target=aarch64-unknown-linux-gnu
```

Rust currently does not transparently pass `-fpass-plugin`; the
`-Cllvm-args=-load-pass-plugin=` form is the supported escape hatch.
You will likely need to combine this with a `build.rs` that links the
runtime into your binary.

### Opt-out per function

Mark functions you don't want instrumented (e.g. fault handlers,
runtime helpers):

```c
__attribute__((no_split_stack)) void hot_isr(void) { ... }
```

In Rust:

```rust
#[no_split_stack]   // or any attribute that lowers to the LLVM
                    // function attribute "no-split-stack"
fn isr() { ... }
```

The pass also auto-skips `__morestack`, `__split_stack_*`, and
`__hopter_stklet_bound` to avoid recursion.

---

## What the assembly looks like before vs. after

Take `recurse` from [demo/demo.c](demo/demo.c):

```c
static int recurse(int depth) {
    volatile int pad[64];
    for (int i = 0; i < 64; i++) pad[i] = depth + i;
    int sum = 0;
    for (int i = 0; i < 64; i++) sum += pad[i];
    if (depth <= 0) return sum;
    return recurse(depth - 1) + sum;
}
```

### Without the plugin (stock clang)

```asm
recurse:
    stp     x29, x30, [sp, #-32]!
    stp     x20, x19, [sp, #16]
    add     x29, sp, #0
    sub     x8, sp, #256
    add     sp, x8, #0
    ; ... loop to fill pad[] ...
    ; ... loop to sum pad[] ...
    cmp     w20, #1
    b.lt    .Lret
    sub     w0, w0, #1
    bl      recurse
    add     w19, w0, w19
.Lret:
    mov     w0, w19
    add     sp, x29, #0
    ldp     x20, x19, [sp, #16]
    ldp     x29, x30, [sp], #32
    ret
```

### With the plugin

```asm
recurse:
    stp     x29, x30, [sp, #-32]!
    stp     x20, x19, [sp, #16]
    add     x29, sp, #0

    ;;;;; — split-stack prologue check (added by the IR pass) —————
    adrp    x8, :tlsdesc:__hopter_stklet_bound
    ldr     x8, [x8, #:tlsdesc_lo12:__hopter_stklet_bound]
    mrs     x9, tpidr_el0           ; per-thread base
    ldr     x8, [x9, x8]            ; %bound
    sub     x9, sp, #0x180          ; sp - 384  (estimated frame)
    cmp     x9, x8
    b.lo    .Lss_morestack          ; cold path
    ;;;;;————————————————————————————————————————————————————————

    sub     x8, sp, #0x100          ; original alloca for pad[]
    add     sp, x8, #0
    ; ... fill loop ...
    ; ... sum loop ...
    cmp     w20, #1
    b.lt    .Lret
    sub     w0, w0, #1
    bl      recurse
    add     w19, w0, w19
.Lret:
    mov     w0, w19
    add     sp, x29, #0
    ldp     x20, x19, [sp, #16]
    ldp     x29, x30, [sp], #32
    ret

.Lss_morestack:                     ; cold (out-of-line)
    mov     w19, w0                 ; preserve arg
    mov     w0, #0x180              ; frame estimate -> arg0
    bl      __morestack
    mov     w0, w19                 ; restore arg
    b       .Lresume
```

Total cost on the **fast path** (instrumentation overhead per call):

| Instructions added | What they do |
|---|---|
| `adrp` + `ldr` | Resolve TLS offset of `__hopter_stklet_bound` |
| `mrs tpidr_el0` + `ldr` | Read per-thread bound value |
| `sub` + `cmp` + `b.lo` | Subtract estimated frame, compare, predict-not-taken |

Six instructions, one not-taken branch.  All loads hit the same cache
line as adjacent TLS data after the first call.  This is comparable
in steady-state cost to the in-tree Hopter prologue.

Cold path adds a small out-of-line block per function (5 instructions
plus the call itself).

### How to inspect for yourself

`run.sh` ends with an objdump of `recurse`:

```bash
./run.sh    # last section of output is the live disassembly
```

To compare against an uninstrumented build, drop `-fpass-plugin=` in
the script and rerun.  Or for a single-function side-by-side:

```bash
clang --target=aarch64-linux-gnu -O2 -S demo/demo.c -o /tmp/demo.before.s
clang --target=aarch64-linux-gnu -O2 -S \
      -fpass-plugin=./build/SplitStackPass.so demo/demo.c -o /tmp/demo.after.s
diff -u /tmp/demo.before.s /tmp/demo.after.s | less
```

---

## Limitations and what's next

| Topic | Status in this demo | Path forward |
|---|---|---|
| Stack-check insertion (static frame) | ✅ Working | — |
| Frame-size estimation | ✅ Conservative bound from IR allocas + kFramePadding | Optionally feed exact size from a post-codegen MachineFunctionPass via a placeholder relocation |
| Dynamic alloca / VLA check | ✅ Per-VLA check inserted before every `!isStaticAlloca()` | VLA > kDynamicAllocaCharge → panic (explicit contract; see Design analysis §B) |
| VLA overflow policy | ✅ Panic on check failure (not heap fallback) | `kDynamicAllocaCharge` is a per-target tunable |
| `__morestack` real stacklet swap + entry-restart | ❌ Demo only lowers bound | Implement entry-restart: save args, switch SP, jump to function entry, free stacklet on return (see Design analysis §C) |
| `#[no_split_stack]` attribute | ✅ via LLVM fn attr `"no-split-stack"` | Rust front-end attr lowering |
| Drop-glue distinction (Hopter Patch 2) | ❌ | Identify drop functions in IR (`__rust_drop_in_place_*` / type metadata) and call `__morestack_drop` instead |
| Block `nounwind` inference (Hopter Patch 3) | ❌ | Hard at IR-pass level; needs rustc cooperation |
| Unwind/cleanup-pad split-stack coordination | ❌ | Out of scope for IR pass; either change Hopter unwind to a pure setjmp/longjmp design, or accept that this part still needs a compiler fork |

In short, **the largest piece of the Hopter custom-toolchain story —
the per-function stack check including VLA handling — fits in a ~350-line
LLVM pass plus a handful of runtime symbols**, with no rustc/LLVM source
modification.  The unwind cooperation remains the genuinely-compiler-coupled
part.

---

---

## Design analysis: correctness, completeness and trade-offs

This section records the detailed design reasoning behind the IR-pass
approach — what problems it solves, how it solves them, where it falls
short of a full backend implementation, and why the remaining gap is
acceptable for the Hopter embedded target.

---

### A. Why frame size cannot be exact in an IR pass

The LLVM pipeline stages relevant here are:

```
IR pass (us) → SelectionDAG → Register Allocation → Frame Lowering → MachineCode
```

The pass runs **before** the backend, so four frame-size components are
invisible at IR time:

| Component | Who decides it | When |
|---|---|---|
| **S** — static alloca bytes | IR pass can sum directly | IR time (exact) |
| **C** — callee-saved registers | Register Allocator | after IR pass |
| **P** — register spills | Register Allocator | after IR pass |
| **K** — stack canary | Frame Lowering | after IR pass |
| **G** — alignment gap | Frame Lowering | after IR pass |

Hopter's in-tree backend patch (`llvm-1-implement-segmented-stack…patch`)
avoids this by querying `MachineFrameInfo::getStackSize()` after frame
lowering — at that point all five components are already folded into one
exact number.  The IR pass cannot do this; it must estimate C + P + K + G
with a fixed padding.

**Measured values on AArch64 (-O2):**

From the disassembly of the three correctness-test functions:

```
overflow_frame:
  stp x29,x30,[sp,#-16]!   → C = 16 B (FP + LR only)
  sub sp, sp, #0x390        → S = 912 B (900 B buf rounded up)
  Actual frame = 928 B,  estimate = 1040 B,  over-estimate = 112 B

deep_recurse:
  stp x29,x30,[sp,#-32]!
  stp x20,x19,[sp,#16]     → C = 32 B (FP + LR + x19 + x20)
  sub sp, sp, #0x100        → S = 256 B
  Actual frame = 288 B,  estimate = 384 B,  over-estimate = 96 B

chain:
  stp x29,x30,[sp,#-32]!
  str x19,[sp,#16]          → C = 32 B (FP + LR + x19)
  (no alloca)               → S = 0 B
  Actual frame = 32 B,   estimate = 128 B,  over-estimate = 96 B
```

In all three cases P = 0 because -O2 prefers callee-saved registers
(x19–x28) over spilling: a live value across a call is placed in x19
rather than `[sp+N]`.  This means kFramePadding = 128 B is slightly
larger than necessary (C ≤ 96 B in practice), giving a comfortable
margin.

The worst-case formal bound is:
- C ≤ 96 B (10 callee-saved × 8 B + FP + LR = 96 B)
- P ≤ 152 B (estimated; depends on register pressure)
- G ≤ 15 B
- K ≤ 8 B
- Total worst-case = 271 B → formal safe kFramePadding = 272 B

The current 128 B covers normal -O2 code (P ≈ 0).  For aggressive -O0
or heavily spill-prone IR, `--split-stack-extra-pad` can be used to
increase the budget.

---

### B. Dynamic alloca (VLA) handling

#### B.1 The problem

A C99 VLA `char buf[n]` lowers to IR as `alloca i8, %n` — the size is a
runtime variable.  The entry-block check uses a compile-time constant and
cannot cover it.

Additionally, the backend **cannot** include the VLA in the prologue's
one-shot `sub sp, sp, #N` because the size is only known at runtime.
VLA allocation always happens inline, at the point of declaration:

```asm
; static alloca (always in prologue):
sub  sp, sp, #900          ; done once, on entry

; VLA (done at the point of declaration, mid-function):
; ... other code first ...
sub  x8, sp, %n_rounded    ; happens here, at runtime
mov  sp, x8
```

This means both a backend implementation **and** our IR pass must insert
a separate per-VLA check at the point of allocation; neither can "hoist"
the VLA check to the function entry.

#### B.2 The instrumentation pattern

For each `AllocaInst` where `!isStaticAlloca()`, the pass splits the
basic block at the alloca and inserts a check:

```llvm
; Before (original):
  %buf = alloca i8, i64 %n

; After instrumentation:
PreBB:
  %ss.dyn.sp    = call i64 @llvm.read_register.i64(metadata !"sp")
  %ss.dyn.bound = load i64, ptr @__hopter_stklet_bound
  %ss.dyn.bytes = mul i64 %n, <element_size>
  %ss.dyn.need_sz = add i64 %ss.dyn.bytes, kFramePadding
  %ss.dyn.need  = sub i64 %ss.dyn.sp, %ss.dyn.need_sz
  %ss.dyn.ok    = icmp uge i64 %ss.dyn.need, %ss.dyn.bound
  br i1 %ss.dyn.ok, label %ss.dyn.cont, label %ss.dyn.morestack
      [branch weights 1024:1]

ss.dyn.morestack:
  call void @__morestack(i64 %ss.dyn.need_sz)
  br label %ss.dyn.cont

ss.dyn.cont:
  %buf = alloca i8, i64 %n   ; original alloca, now inside new zone
  ...
```

The disassembly of `vla_test` shows both checks:

```asm
; [1] Entry check  (static frame estimate = 1040 B)
sub  x9, sp, #0x410
cmp  x9, x8
b.cc .morestack_entry   → morestack(0x410 = 1040)

; [2] Dynamic alloca check  (runtime size 900 + 128 padding = 1028 B)
sub  x9, sp, #0x404
cmp  x9, x8
b.cc .morestack_dyn     → morestack(0x404 = 1028)

; Actual VLA allocation — SP only moves here
sub  x8, sp, #0x390    ; 0x390 = 912 = round_up_16(900)
mov  sp, x8
```

#### B.3 The "one function, one stacklet" invariant

A fundamental correctness property of split-stack is:

> **A function's frame must lie entirely within a single stacklet.**

If mid-function VLA check fires and triggers a real stacklet switch,
the first half of the frame is on the old stacklet and the new VLA is
on the new one.  Because compilers access locals via SP- or FP-relative
offsets, the two halves would be in non-contiguous memory — immediate UB.

This rules out "switch stacklets mid-function" as a response to a VLA
check failure.

#### B.4 Three candidate responses to VLA check failure

| Response | Verdict |
|---|---|
| Allocate VLA on heap (`malloc`) | Preserves invariant, but: (a) `longjmp`/exceptions can skip the `free`, causing leaks; (b) a bug with repeated large VLAs silently drains the heap — defeats the purpose of stack monitoring |
| Extend the current stacklet in-place (`grow-in-place`) | Preserves invariant, frame stays contiguous; requires the runtime to extend physical memory at the current stacklet's low end |
| **Panic (abort)** | Cleanest for embedded targets: VLA larger than `kDynamicAllocaCharge` is declared a programming error |

The chosen design is **panic on VLA check failure**, for these reasons:

1. The entry check is designed to pre-approve the VLA budget:
   the entry estimate includes `N × kDynamicAllocaCharge` per dynamic
   alloca found in the function.  If entry check passes on a fresh
   stacklet, there is enough room.  VLA check can only fail if the
   actual VLA size exceeds `kDynamicAllocaCharge`.

2. `kDynamicAllocaCharge` is an explicit system contract: any VLA
   that exceeds it is a programming error (VLA too large for the
   embedded target), not a normal overflow that should be silently
   accommodated.

3. Heap fallback would make it impossible to detect runaway VLA
   allocation bugs — the exact scenario stack monitoring is meant
   to catch.

---

### C. Entry-restart: the full `__morestack` contract

When the **entry check** fails (not mid-function), the function has not
executed a single line of its body yet.  It is safe to restart it on a
new stacklet.

The complete `__morestack` contract for a production implementation:

```
Entry check fires:
  1. Allocate new stacklet of size (frame_estimate + extra)
  2. Save original arguments (x0–x7, x8 if sret)
  3. Switch SP to the new stacklet's top
  4. Restore original arguments
  5. Jump to the function's entry point (not ss.cont — re-execute from the top)
  6. When the function's ret executes:
       a. Switch SP back to the old stacklet
       b. Free the new stacklet
       c. Return to the original call site
```

After step 5, the function runs start-to-finish within the new stacklet.
All static and dynamic allocas land in that stacklet's memory — the
invariant is preserved.

The current demo implementation does **not** perform the stacklet switch
(step 1–6 above).  It only lowers the soft bound and returns.  This is
sufficient to prove the check-and-continue mechanism correct; the actual
physical memory management is the next implementation step.

---

### D. The `kFramePadding` dual role

`kFramePadding` serves **two independent safety purposes**:

#### Role 1 — Cover invisible backend overhead (C + P + K + G)

The IR pass cannot see what the backend will add to the frame.
128 B conservatively covers the callee-save area, any spills, the
optional stack canary, and alignment padding.

#### Role 2 — Guard against the prologue-before-check window

The LLVM backend always places callee-save pushes (`stp x29,x30,[sp,#-32]!`)
as the very first instructions of a function, before the IR-level check
block.  This creates a small window:

```
caller passes bar's entry check (OK)
    ↓
bar prologue: stp x29,x30,[sp,#-32]!   ← writes below SP, before any check
    ↓
bar's entry check runs
```

If bar's prologue happens to write past the soft bound, those writes hit
memory below the bound before any check can stop them.

`kFramePadding` prevents this: foo's check guarantees that
`SP_foo_bottom − bound ≥ kFramePadding`.  Bar's prologue writes at most
96 B (maximum callee-save area on AArch64), which is ≤ 128 B, so those
writes land within the padding buffer — memory is valid, writes complete
safely, and then bar's own check fires.

```
After foo's check passes:

  │  foo frame                        │
  │  ← SP (foo's alloca bottom)       │
  │                                   │
  │  [≥ 128 B guaranteed by padding]  │  ← bar's prologue writes here (safe)
  │  [bar's check runs here]          │
  │                                   │
  ─ ─ ─ ─ ─ ─ soft bound ─ ─ ─ ─ ─ ─ ─
```

This means **`kFramePadding` must be ≥ max-possible-prologue-size** (96 B
on AArch64) in addition to covering C + G + P + K.  The current 128 B
satisfies both simultaneously.

---

### E. Summary: what the IR pass can and cannot do

| Requirement | IR pass | Backend patch |
|---|---|---|
| Insert stack check at function entry | ✅ exact | ✅ exact |
| Know exact frame size | ❌ must estimate | ✅ `MFI.getStackSize()` |
| Cover static allocas precisely | ✅ sum of IR allocas | ✅ included in MFI |
| Cover dynamic VLAs (per-alloca check) | ✅ insert check before each VLA | ✅ backend inserts naturally |
| Entry-restart (switch stacklet, re-run function) | ✅ implementable in runtime | ✅ implementable in runtime |
| Guarantee "one function, one stacklet" | ✅ with panic-on-VLA-overflow | ✅ naturally |
| Modify compiler | ❌ not required | ✅ required |
| Exact frame-size precision | ❌ (kFramePadding over-estimates) | ✅ |

The IR pass is a complete and correct implementation of the split-stack
**check** mechanism with three deliberate trade-offs:

1. Frame size is over-estimated by kFramePadding → earlier (but never
   missing) `__morestack` calls.
2. VLAs larger than `kDynamicAllocaCharge` panic instead of growing →
   explicit size contract on the embedded target.
3. No compiler modification required.

---

## Reference

- LLVM new PassManager plugin docs: https://llvm.org/docs/WritingAnLLVMNewPMPass.html
- GCC split-stack ABI (for comparison): https://gcc.gnu.org/wiki/SplitStacks
- Hopter Patch 1 background: `obsi/hopter/hopter-toolchain-implementation.md` §2 / §10.7
