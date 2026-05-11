# aarch64 分段栈插件（Hopter 风格，无需修改编译器）

本项目以概念验证的方式表明：**Hopter 的分段栈机制可以在 aarch64 上实现，而无需修改 rustc/LLVM**——只需提供一个小型的树外 LLVM IR Pass 以及一个用户态运行时即可。

该 Pass 在每个函数入口处插入栈边界检查；在冷路径上调用运行时辅助函数 `__morestack`。与 Hopter 的树内方案（自定义 LLVM/rustc fork）相比，本插件的优势在于：

- **无需重新构建工具链。** 唯一的产物是一个 50 KB 的 `.so` 动态库。
- **无需修改后端。** 可在原版 clang 18 上运行。
- **Pass 本身具备架构可移植性。** 同一份 C++ 代码可为 clang 支持的任意 aarch64/riscv/x86 后端服务；只有运行时需要关心目标平台的 SP/TLS 约定。

与真正的编译器补丁相比，本方案的取舍在于：
- **帧大小是估算值**，而非精确值（过估只会导致 `__morestack` 被更早调用，永远不会引发未定义行为）。
- **未实现 unwind/cleanup-pad 协调**——IR 级 Pass 无法像 rustc/LLVM 补丁那样拦截 EH personality 和 `cleanuppad` 代码生成。本演示专注于 Hopter 设计中的栈检查部分。

---

## 目录

1. [总体软件设计](#总体软件设计)
2. [仓库结构](#仓库结构)
3. [快速开始](#快速开始)
4. [测试结果](#测试结果)
5. [核心设计](#核心设计)
   - [插桩合约](#1-插桩合约)
   - [帧大小估算](#2-帧大小估算)
   - [动态 alloca（VLA）处理](#3-动态-allocavla-处理)
   - [`__morestack`——零栈用量实现](#4-__morestack零栈用量实现)
   - [运行时合约](#5-运行时合约)
   - [Pass 注册机制](#6-pass-注册机制)
6. [汇编对比](#汇编对比)
7. [集成指南](#集成指南)
8. [局限性与后续工作](#局限性与后续工作)
9. [深度设计分析](#深度设计分析)
10. [参考资料](#参考资料)

---

## 总体软件设计

下图展示了本插件的三大组件及其协作关系：

```
┌─────────────────────────────────────────────────────────────────────┐
│                        编译阶段（Build Time）                         │
│                                                                       │
│   源代码 (demo.c / foo.rs)                                            │
│        │                                                              │
│        ▼                                                              │
│   clang -O2 -fpass-plugin=SplitStackPass.so                           │
│        │                                                              │
│        ├── LLVM IR 优化管线（inline / DCE / …）                        │
│        │                                                              │
│        │   ┌──────────────────────────────────────────────┐          │
│        │   │         SplitStackPass（IR Pass）             │          │
│        │   │                                              │          │
│        │   │  对每个函数：                                  │          │
│        │   │  1. 在入口块插入一次检查：                     │          │
│        │   │     后端序言已把所有静态帧写入 SP，             │          │
│        │   │     直接读取当前 SP：                          │          │
│        │   │     (SP - kFramePadding) >= bound ?          │          │
│        │   │  2. 对每个动态 alloca(VLA)，在分配指令之后     │          │
│        │   │     立即插入检查（形式与入口完全相同）           │          │
│        │   └──────────────────────────────────────────────┘          │
│        │                                                              │
│        └── aarch64 目标代码生成                                        │
│                       │                                               │
│                       ▼                                               │
│             插桩后的目标文件 (.o)                                       │
│                       │                                               │
│         ┌─────────────┴──────────────┐                               │
│         ▼                            ▼                               │
│   runtime/runtime.c          runtime/runtime_aarch64.S               │
│  ┌───────────────────────┐  ┌──────────────────────────────┐        │
│  │ 提供全局变量：         │  │ __morestack 汇编实现          │        │
│  │                       │  │                              │        │
│  │ __hopter_stklet_bound │  │ • 切换至紧急栈（64 KiB 静态   │        │
│  │   (TLS，每线程独立)    │  │   缓冲区），然后 bl 调用 C    │        │
│  │ __overflow_emergency  │  │   overflow 处理函数           │        │
│  │   _stack（64 KiB 静态 │  │ • 处理函数打印诊断信息并      │        │
│  │   紧急栈）            │  │   abort()                    │        │
│  │ __split_stack_ctor    │  │ • x29/x30/x19-x28 切换前     │        │
│  │   (构造函数，设初始bound)│  │   全程不变                   │        │
│  │ __morestack_overflow  │  └──────────────────────────────┘        │
│  │   (noreturn，诊断+abort)│                                          │
│  └───────────────────────┘                                           │
│         │                            │                               │
│         └─────────────┬──────────────┘                               │
│                       ▼                                               │
│               链接为最终可执行文件                                       │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                        运行阶段（Runtime）                             │
│                                                                       │
│  程序启动                                                              │
│      │                                                                │
│      ▼                                                                │
│  __split_stack_ctor（构造函数，在 main() 之前自动执行）                  │
│      │  bound = init_SP - max_stack + kGuardMargin                   │
│      │  默认 max_stack = 2 MB（可通过 HOPTER_MAX_STACK_KB 覆盖）        │
│      │                                                                │
│      ▼                                                                │
│  每次函数调用（已插桩）                                                  │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  ① 后端生成的序言（Pass 之前）                                    │ │
│  │       stp x29, x30, [sp, #-32]!   ← callee-save 压栈           │ │
│  │                                                                  │ │
│  │  ② 栈边界检查（Pass 插入，读取的是序言后的实际 SP）                 │ │
│  │       %sp    = llvm.read_register("sp")  ← 已含所有静态帧        │ │
│  │       %bound = load @__hopter_stklet_bound                      │ │
│  │       %need  = sp - kFramePadding(128B)                         │ │
│  │       if need >= bound ─────────────────────► 热路径（>99%）     │ │
│  │       else ─────────────────────────────────► 冷路径            │ │
│  │                                                    │             │ │
│  │  ③ 函数体正常执行                                    │             │ │
│  │       sub sp, sp, n   ← 动态 VLA 分配（若有）        │             │ │
│  │       ↓ 每个 VLA 后也有相同形式的检查                 │             │ │
│  └────────────────────────────────────────────────────┼─────────────┘ │
│                                                        │               │
│                                           ┌────────────┘               │
│                                           ▼                            │
│                                    __morestack（汇编）                  │
│                              ┌──────────────────────────────────────┐ │
│                              │ sp = __overflow_emergency_stack top  │ │
│                              │   （切换至 64 KiB 紧急栈）            │ │
│                              │ bl __morestack_overflow              │ │
│                              │ ↓（永不返回）                         │ │
│                              │ __morestack_overflow:                │ │
│                              │   fprintf(诊断信息)                  │ │
│                              │   abort()                           │ │
│                              └──────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘

关键接口符号：
  __hopter_stklet_bound       — TLS 变量，每线程独立的栈软边界
  __morestack                 — 检测到溢出时：切换至紧急栈，调用 overflow 处理函数
  __overflow_emergency_stack  — 64 KiB 静态缓冲区，__morestack 用作紧急栈
  __morestack_overflow        — 打印诊断信息并 abort()（noreturn）
```

---

## 仓库结构

| 路径 | 用途 |
|---|---|
| [pass/SplitStackPass.cpp](pass/SplitStackPass.cpp) | LLVM IR Pass（新 PassManager 插件） |
| [runtime/runtime.c](runtime/runtime.c) | 连续栈运行时——设置软边界，提供溢出处理函数 |
| [runtime/runtime_aarch64.S](runtime/runtime_aarch64.S) | `__morestack` 汇编：切换至紧急栈后 abort |
| [demo/demo.c](demo/demo.c) | 主演示：正常函数调用，不触发溢出 |
| [demo/correctness_test.c](demo/correctness_test.c) | 正确性测试：验证无假阳性 |
| [demo/overflow_demo.c](demo/overflow_demo.c) | 溢出演示：故意触发溢出，预期 abort |
| [CMakeLists.txt](CMakeLists.txt) | 基于 `/usr/lib/llvm-18` 构建 Pass |
| [build.sh](build.sh) | 一键 CMake 构建插件 |
| [run.sh](run.sh) | 交叉编译演示程序，在 qemu-aarch64 下运行并输出反汇编 |

---

## 快速开始

```bash
./build.sh                      # 构建 build/SplitStackPass.so
./run.sh                        # 交叉编译三个 demo，在 qemu-aarch64 下运行
SPLIT_STACK_VERBOSE=1 ./run.sh  # 同上，并打印每个函数的帧估算
HOPTER_MAX_STACK_KB=512 ./run.sh  # 用 512 KiB 最大栈重新测试
```

测试环境：
- Ubuntu 22.04（宿主机）
- clang/LLVM 18.1.8（Debian 包：`clang-18 / llvm-18-dev / lld-18`）
- qemu 8.0.2（`qemu-aarch64`）

> 整个构建链（编译、汇编、链接、反汇编）均使用 LLVM 工具链完成，不依赖 `gcc-aarch64-linux-gnu`。链接器使用 `lld`（通过 `-fuse-ld=lld` 指定），反汇编使用 `llvm-objdump`。

---

## 测试结果

所有测试均在 x86-64 宿主机上通过 `qemu-aarch64 8.0.2`（Linux 用户态仿真）运行。

### 主演示（`demo/demo.c`）

```
[hopter] init SP=0x5502822b80  max_stack=2048 KB
[hopter] bound=0x5502624b80  (SP - max_stack + 8192 B guard)
=== shallow ===
=== big_frame ===
=== recurse(20) ===
results: a=175 b=261120 c=55776
[OK] all functions completed without overflow
```

连续栈上所有函数正常运行，没有任何溢出触发。

### 正确性测试（`demo/correctness_test.c`）

验证插桩检查不干扰计算结果（无假阳性）：

```
[hopter] init SP=0x5502822b70  max_stack=2048 KB
[hopter] bound=0x5502624b70  (SP - max_stack + 8192 B guard)

=== Correctness test ===
[PASS] large_frame: result=106566 (expected 106566)
[PASS] deep_recurse(29): result=435 (expected 435)
[INFO] chain: result=106997
[PASS] vla_test(900): result=900 (expected 900)

[OK] all checks passed — no false positives
```

| 测试用例 | 结果 | 状态 |
|---|---|---|
| `large_frame`（900 B 静态缓冲区） | 106566 | **PASS** |
| `deep_recurse(29)`（30 × 256 B ≈ 8 KiB） | 435 | **PASS** |
| `chain`（两者组合） | 106997 | INFO |
| `vla_test(900)`（VLA，动态 alloca） | 900 | **PASS** |

### 溢出演示（`demo/overflow_demo.c`，预期 abort）

故意以 4 KiB/帧的速度递归，直到触发软边界：

```
[hopter] init SP=0x5502822b70  max_stack=2048 KB
[hopter] bound=0x5502624b70  (SP - max_stack + 8192 B guard)
[overflow_demo] starting deep recursion (4 KiB/frame)...
[overflow_demo] expect overflow after ~490 frames
[overflow_demo] depth=0
...
[overflow_demo] depth=500

[hopter] *** STACK OVERFLOW DETECTED ***
[hopter]   soft bound  = 0x5502624b70
[hopter]   init SP     = 0x5502822b70
[hopter]   max stack   = 2048 KB
qemu: uncaught target signal 6 (Aborted) - core dumped
```

在约 490–510 帧时触发，诊断信息清晰，进程随后 abort。

关键观察：
- 溢出在真正越过物理栈底**之前**被检测到（软边界比物理底部高 8 KB）。
- 与系统默认的 guard page（精度为页，4 KiB）相比，软件检查在**每个函数入口**提前拦截。
- 标准 DWARF unwinder 无需修改即可工作——连续栈保证了展开路径的完整性。

---

## 核心设计

### 1. 插桩合约

对于每个已定义的函数 `F`（跳过声明、naked 函数以及标注了 `"no-split-stack"` 属性的函数），Pass 将入口块从：

```text
entry:
    <原始指令>
```

改写为：

```text
entry:                                            ; 检查块
    %sp    = call i64 @llvm.read_register.i64(metadata !"sp")
    %bound = load i64, i64* @__hopter_stklet_bound   ; 线程局部变量
    %need  = sub i64 %sp, <估算帧大小>
    %ok    = icmp uge i64 %need, %bound
    br i1 %ok, label %ss.cont, label %ss.morestack
        !prof !{!"branch_weights", i32 1024, i32 1}  ; 冷路径提示

ss.morestack:
    call void @__morestack(i64 <估算帧大小>)
    br label %ss.cont

ss.cont:
    <原始指令>
```

Pass 引入两个**弱外部**接口符号：

| 符号 | 类型 | 由谁提供 |
|---|---|---|
| `__hopter_stklet_bound` | `thread_local u64` | 运行时 |
| `__morestack` | `void(u64)` | 运行时 |

Pass 还在每个插桩后的函数上标记两个属性，供检查和后续 Pass 使用：

```
attributes = { "split-stack-instrumented" "split-stack-frame-estimate"="128" }
```

### 2. 检查形式：统一的两字指令

本插件的核心简化思路是：**不需要在 Pass 里估算帧大小**。

编译器后端（SelectionDAG → 帧降低）负责生成函数序言（prologue），其中的 `stp / sub sp` 系列指令在函数体任何 IR 指令之前执行，已经将 SP 精确地移动到包含了所有静态 alloca、callee-save 寄存器、spill 槽位等的位置。IR Pass 在序言之后读取 SP，得到的就是真实的、已分配好的栈顶。

因此，**入口检查**只需两件事：

```
┌──────────────────────────────────────────────────────┐
│  post_prologue_SP  ─  kFramePadding  >=  bound ?     │
│  ↑ 后端已处理好    ↑ 固定常量 128B   ↑ TLS 变量      │
└──────────────────────────────────────────────────────┘
```

对应 IR：

```llvm
%sp    = call i64 @llvm.read_register.i64(metadata !"sp")
%bound = load i64, ptr @__hopter_stklet_bound
%need  = sub i64 %sp, 128       ; kFramePadding，仅此而已
%ok    = icmp uge i64 %need, %bound
br i1 %ok, %ss.cont, %ss.morestack
```

**Pass 不再遍历任何 alloca**，也不需要 `estimateFrameSize`。

### 3. 动态 alloca（VLA）处理

动态 alloca（`char buf[n]`）在后端同样降低为运行时的 `sub sp, sp, n` 指令，与静态 alloca 的处理对称——分配后 SP 立即反映了实际分配量。

因此对 VLA 的检查与入口检查**完全相同**：在 alloca 指令之后读取 post-alloca SP，再减去同一个 `kFramePadding`：

```
┌──────────────────────────────────────────────────────┐
│  post_alloca_SP  ─  kFramePadding  >=  bound ?       │
└──────────────────────────────────────────────────────┘
```

两种检查统一成一种形式，Pass 不需要知道 VLA 的运行时大小。

**SP 短暂越过 bound 的安全性**

VLA 分配后、检查运行前，SP 可能已经低于 bound：

```
sub sp, sp, n   ← SP 下移，可能越过 bound
                ← 窗口：SP < bound，但无内存写操作
ldr x8, [tpidr] ← 读 TLS（只读）
sub x9, sp, 128 ← 算术（只用寄存器）
cmp x9, x8
b.lo morestack  ← 发现越界，abort
```

bound 是软件约定边界，其下方是合法映射内存（guard margin + 紧急栈），越过它不触发硬件异常。窗口内没有任何写操作，因此不会发生内存损坏。

**插桩后的 IR 形态（每个 VLA）：**

```llvm
ss.dyn.alloca:
  %buf = alloca i8, i64 %n        ; SP 在此下移
  %sp  = call i64 @llvm.read_register.i64(metadata !"sp")
  %bound = load i64, ptr @__hopter_stklet_bound
  %need  = sub i64 %sp, 128
  %ok    = icmp uge i64 %need, %bound
  br i1 %ok, %ss.dyn.cont, %ss.dyn.morestack

ss.dyn.morestack:
  call void @__morestack(i64 128)
  br %ss.dyn.cont

ss.dyn.cont:
  ; 继续使用 %buf
```



### 4. `__morestack`——溢出检测实现

#### 4.1 执行过程中各关键时刻的栈布局

**时刻 A——函数入口，序言之前**

```
高地址 ──────────────────────────────────────
      │  调用者帧                             │
      └───────────────────────────────────────┘
         ↑ SP（= 调用者的 SP，bl 指令不移动 SP）
低地址  ─ ─ ─ ─ ─ ─ ─ ─ bound ─ ─ ─ ─ ─ ─ ─ ─
```

**时刻 B——序言执行后**

LLVM 后端始终将 callee-save 压栈作为函数体的第一批指令，在任何 IR 级检查之前执行。

```
高地址 ────────────────────────────────────────────
      │  调用者帧                                   │
      └──────────────────────────────────────────────┘
      │ saved x22/x21/x20/x19                       │
      │ saved x29/x30       │ ← SP（序言写入所有静态帧）
低地址  ─ ─ ─ ─ ─ ─ ─ ─ bound ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
      │  此区域未被写入                               │
```

**时刻 C——IR 检查执行（post-prologue SP - kFramePadding < bound）**

```
高地址 ────────────────────────────────────────────
      │  调用者帧                                   │
      └──────────────────────────────────────────────┘
      │ saved x29/x30/x19-x22                       │
      │                     │ ← SP（检查不动 SP）
低地址  ─ ─ ─ ─ ─ ─ ─ ─ bound ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
      │  SP - 128 落在此处   │ ← 触发检查的位置
      │  < bound → 触发 __morestack                  │
```

**时刻 D——`__morestack` 内部**

`bl __morestack` 写入 x30，但**不移动 SP**。SP 仍指向达到溢出点的函数栈顶。为了安全地调用 `fprintf`/`abort`，实现将 SP 切换至静态紧急栈：

```
高地址 ────────────────────────────────────────────
      │  调用者帧                                   │
      └──────────────────────────────────────────────┘
      │ saved x29/x30/x19-x22                       │
      │                     │ ← SP（__morestack 入口时）
低地址  ─ ─ ─ ─ ─ ─ ─ ─ bound ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
      │  SP - 128 落在此处   │ ← 触发检查的位置

（切换到 __overflow_emergency_stack 顶部，永不返回）
```

alloca 不会执行，进程随后 abort。

#### 4.2 序言先于检查的安全性

在新方案中，**入口检查读取的就是序言后的实际 SP**，序言写入内存是安全的，原因是调用者的检查为被调函数的序言写操作"托底"（详见 §B）。

- 检查通过（无需 morestack）：函数体继续执行，一切在当前连续栈内。
- 检查失败：触发溢出处理，进程 abort。

两种情况下，均不会有任何写操作越过软边界。

#### 4.3 实现：切换 SP 到紧急栈，bl 到 C 处理函数

```asm
__morestack:
    adrp    x1, __overflow_emergency_stack
    add     x1, x1, #:lo12:__overflow_emergency_stack
    add     x1, x1, #65536     // SP = 64 KiB 缓冲区顶部
    mov     sp, x1
    bl      __morestack_overflow  // noreturn
0:  b       0b                    // 不可达
```

`__morestack_overflow`（runtime.c，`noreturn`）打印诊断信息并 `abort()`：

```c
[hopter] *** STACK OVERFLOW DETECTED ***
[hopter]   soft bound  = 0x5502624b70
[hopter]   init SP     = 0x5502822b70
[hopter]   max stack   = 2048 KB
```

#### 4.4 Local Exec TLS 为何能避免函数调用

`__hopter_stklet_bound` 是 `__thread uint64_t`，三种 TLS 访问模型的对比：

| 模型 | 生成代码 | 是否函数调用 |
|---|---|---|
| General Dynamic（`.so` 默认） | `bl __tls_get_addr` | **是——被禁止** |
| Initial Exec | `adrp + ldr [GOT] + mrs + add` | 否 |
| **Local Exec**（本项目使用） | `mrs tpidr_el0 + add + add` | **否** |

Local Exec 适用于 TLS 变量与代码在同一链接单元的可执行文件。线程指针偏移由静态链接器在链接时写入，运行时无需解析。

IR Pass 在插桩调用者代码时使用 General Dynamic（适用于任何上下文）。`__morestack` 汇编中对 `__hopter_stklet_bound` 的访问使用 Local Exec——安全，因为 `runtime_aarch64.S` 始终被静态链接到最终可执行文件中。

#### 4.5 寄存器不变量

| 寄存器 | `__morestack` 入口时 | `bl __morestack_overflow` 时 |
|---|---|---|
| `x0` | frame_estimate（参数，未使用） | 不变 |
| `x1`, `x2` | 调用者保存的临时寄存器 | 被修改 |
| `x19`–`x28` | 调用者的活跃値 | 不变（从未触碰） |
| `x29` | 调用者的 FP | 不变 |
| `x30` | LR → `ss.cont` | 不叓 |
| `sp` | 调用者的 SP（序言后） | **切换到紧急栈** |

> 注意：`__morestack` 内 SP 被修改，但永不返回。对调用者而言 SP 等价于不变（永不返回）。

### 5. 运行时合约

[runtime/runtime.c](runtime/runtime.c) 提供相关全局变量和处理函数：

| 符号 | 类型 | 作用 |
|---|---|---|
| `__hopter_stklet_bound` | `__thread uint64_t` | 软边界，每次调用时检查 |
| `__overflow_emergency_stack` | `char[65536]` | 紧急栈缓冲区，纯静态分配 |
| `__morestack_overflow` | `void(void)` noreturn | 诊断 + abort() |

构造函数 `__split_stack_ctor` 在 `main()` 之前运行，设置初始边界：

```c
uint64_t sp;
__asm__ volatile("mov %0, sp" : "=r"(sp));
s_init_sp = sp;
s_max_stack_bytes = kDefaultMaxStackBytes;   // 2 MB
// 可通过 HOPTER_MAX_STACK_KB 环境变量覆盖
const char *env = getenv("HOPTER_MAX_STACK_KB");
if (env && *env) { unsigned long kb = strtoul(env, NULL, 10); if (kb>0) s_max_stack_bytes = kb*1024UL; }
__hopter_stklet_bound = sp - s_max_stack_bytes + kGuardMargin;  // kGuardMargin = 8 KiB
```

设计原则：软边界比物理栈底高 8 KiB，等价于让检查在真正溢出**之前** 8 KiB 就触发。这一边距大于任何单个帧的最大作用（每帧最多连续消耗数百 KB 往往是超大栈数组等内存布局错误，而非正常递归）。

### 6. Pass 注册机制

Pass 向新 PassManager **注册两次**：

```cpp
PB.registerOptimizerLastEPCallback(...);   // 自动注册，在所有优化之后运行
PB.registerPipelineParsingCallback(...);   // 显式注册，通过 -passes=split-stack 调用
```

在 `OptimizerLastEP` 时机运行，意味着看到的是内联和死代码消除**之后**的 IR——被内联掉的叶函数根本不需要检查。

配置选项：

| 配置项 | 效果 |
|---|---|
| 环境变量 `SPLIT_STACK_VERBOSE=1` | 打印每个函数的帧估算结果 |
| `-mllvm -split-stack-extra-pad=N` | 为每个估算额外添加 N 字节 |
| 函数属性 `"no-split-stack"` | 跳过该函数的插桩 |

---

## 汇编对比

以 [demo/demo.c](demo/demo.c) 中的 `recurse` 为例：

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

### 插件前（原版 clang）

```asm
recurse:
    stp     x29, x30, [sp, #-32]!
    stp     x20, x19, [sp, #16]
    add     x29, sp, #0
    sub     x8, sp, #256
    add     sp, x8, #0
    ; ... 填充 pad[] 的循环 ...
    ; ... 对 pad[] 求和的循环 ...
    cmp     w20, #1
    b.lt    .Lret
    sub     w0, w0, #1
    bl      recurse
    add     w19, w0, w19
.Lret:
    mov     w0, w19
    ldp     x20, x19, [sp, #16]
    ldp     x29, x30, [sp], #32
    ret
```

### 插件后

```asm
recurse:
    ; ── 后端生成的序言（在任何 IR 指令之前）─────────────────────
    stp     x29, x30, [sp, #-0x30]!   ; 保存 6 个 callee-save 寄存器
    stp     x22, x21, [sp, #0x10]     ; SP 共下移 0x30 = 48B
    stp     x20, x19, [sp, #0x20]
    mov     x29, sp

    ; ── 入口检查（IR Pass 插入，读取的是序言后的实际 SP）──────────
    adrp    x19, :got:__hopter_stklet_bound
    sub     x9, sp, #0x80             ; post-prologue SP - 128（kFramePadding）
    ldr     x19, [x19]                ; TLS 偏移
    mrs     x22, tpidr_el0            ; 线程基地址
    ldr     x8, [x22, x19]            ; bound
    cmp     x9, x8
    b.lo    .Lss_morestack            ; 冷路径（预测不跳转）
    ; ─────────────────────────────────────────────────────────────

    ; ── VLA 分配（pad[64]，256B）──────────────────────────────────
    sub     x21, sp, #0x100
    mov     sp, x21                   ; SP 再下移 256B

    ; ── VLA 后检查（形式与入口完全相同）──────────────────────────
    ldr     x8, [x22, x19]            ; 重用已加载的 TLS 偏移
    sub     x9, sp, #0x80             ; post-alloca SP - 128
    cmp     x9, x8
    b.lo    .Lss_dyn_morestack        ; 冷路径
    ; ─────────────────────────────────────────────────────────────
    ; ...
.Lss_morestack:
    mov     w20, w0
    mov     w0, #0x80                 ; 128
    bl      __morestack
    mov     w0, w20
    b       .Lresume
```

关键观察：**两个检查形式完全一致**（`current_SP - 128 >= bound`），Pass 不需要知道任何帧大小。

**热路径开销**（入口新增约 6 条指令，VLA 额外新增约 4 条）：

| 新增指令 | 功能 |
|---|---|
| `adrp` + `ldr` | GOT 加载 TLS 偏移（一次性） |
| `mrs tpidr_el0` + `ldr` | 读取线程局部边界值 |
| `sub` + `cmp` + `b.lo` | SP - kFramePadding 与 bound 比较 |

首次调用后，所有加载命中 TLS 相邻数据的同一缓存行，稳态开销与树内 Hopter 序言相当。

### 如何自行验证

```bash
./run.sh    # 输出末尾即为 recurse 的实时反汇编

# 单函数并排对比
clang --target=aarch64-linux-gnu -O2 -S demo/demo.c -o /tmp/demo.before.s
clang --target=aarch64-linux-gnu -O2 -S \
      -fpass-plugin=./build/SplitStackPass.so demo/demo.c -o /tmp/demo.after.s
diff -u /tmp/demo.before.s /tmp/demo.after.s | less
```

---

## 集成指南

Pass 是标准的新 PassManager 插件，支持三种集成方式：

### 方式 A——直接传递给 clang

```bash
clang -O2 -fpass-plugin=/path/to/SplitStackPass.so foo.c bar.c \
      -o foo  -lsplitstack_rt
```

自行提供运行时符号（`__morestack`、`__hopter_stklet_bound`），例如链接 [runtime/runtime.c](runtime/runtime.c) 或自定义的移植版本。

### 方式 B——通过 `opt` 进行纯 IR 实验

```bash
clang -O2 -emit-llvm -c foo.c -o foo.bc
opt -load-pass-plugin=./SplitStackPass.so -passes=split-stack \
    foo.bc -o foo.instrumented.bc
llc foo.instrumented.bc -o foo.s
```

### 方式 C——Rust（预览，本演示未包含）

```bash
RUSTFLAGS="-Cpasses=split-stack \
           -Cllvm-args=-load-pass-plugin=$PWD/SplitStackPass.so" \
    cargo build --target=aarch64-unknown-linux-gnu
```

Rust 目前不直接支持 `-fpass-plugin`，`-Cllvm-args=-load-pass-plugin=` 是受支持的逃生通道。通常还需要在 `build.rs` 中将运行时链接进二进制文件。

### 按函数退出插桩

```c
// C 中
__attribute__((no_split_stack)) void hot_isr(void) { ... }
```

```rust
// Rust 中
#[no_split_stack]
fn isr() { ... }
```

Pass 还会自动跳过 `__morestack`、`__split_stack_*` 和 `__hopter_stklet_bound`，以避免递归。

---

## 局限性与后续工作

| 方面 | 当前演示状态 | 后续路径 |
|---|---|---|
| 栈检查插入 | ✅ 工作正常 | — |
| 帧大小取得方式 | ✅ 利用序言后的实际 SP，无需估算 | 可通过后端 MachinePass 获取精确大小 |
| 动态 alloca / VLA 检查 | ✅ 每个 `!isStaticAlloca()` 后插入检查 | VLA > kFramePadding → panic（明确契约） |
| VLA 溢出策略 | ✅ 检查失败时 panic（非堆回退） | `kDynamicAllocaCharge` 可按目标调整 |
| 溢出检测与处置 | ✅ 软件检查 + 紧急栈切换 + abort | 嵌入式移植：换为 SVC/trap，不需要 abort 路径 |
| 最大栈大小配置 | ✅ `HOPTER_MAX_STACK_KB` 环境变量 | 嵌入式目标可以在构造函数中直接赋值 |
| `#[no_split_stack]` 属性 | ✅ 通过 LLVM 函数属性 `"no-split-stack"` 实现 | Rust 前端属性降低 |
| 标准 DWARF unwind | ✅ 连续栈保证展开路径完整，无需任何修改 | — |
| Drop glue 区分（Hopter Patch 2） | ❌ | 在 IR 中识别 drop 函数并改调 `__morestack_drop` |
| 阻止 `nounwind` 推断（Hopter Patch 3） | ❌ | IR Pass 级别难以实现，需要 rustc 协作 |

**简而言之：** 每函数栈检查（含 VLA 处理）可用约 350 行 LLVM Pass 加少量运行时符号实现，无需修改 rustc/LLVM 源码。连续栈 + abort 的运行时方案避免了分段栈与 DWARF unwinder 之间的协调问题，代价是不支持真正的动态栈扩展。

---

## 深度设计分析

本节记录 IR Pass 方案背后的详细设计推理——它解决了什么问题、如何解决、在哪些方面不及完整后端实现，以及剩余差距对于 Hopter 嵌入式目标为何可以接受。

### A. IR Pass 与引射阶段的关系

相关的 LLVM 流水线阶段为：

```
IR Pass（本项目）→ SelectionDAG → 寄存器分配 → 帧降低 → 机器码
```

Pass 在后端**之前**运行，因此 callee-save、spill、stack canary、对齐等开销在 IR 时不可见。

**本插件的解决思路：绕开这个问题**。我们不需要在 IR 时知道帧大小，因为当 IR 级检查运行时，序言已经执行完毕——直接读取当前 SP 就得到了包含所有开销的实际值。

Hopter 树内补丁通过在帧降低之后查询 `MachineFrameInfo::getStackSize()` 来获取精确值——本插件利用后端已完成的帧分配输出，就地取用，无需 fork LLVM。
### B. `kFramePadding` 的作用

在新方案中，kFramePadding 只有**一个作用**：为下一个被调函数的序言写操作提供安全窗口。

**序言先于检查的时间窗口**

LLVM 后端始终将 callee-save 压栈（序言）作为函数的第一批机器指令生成，早于 IR Pass 插入的检查代码。因此每次函数调用存在一段**无保护窗口**：

```
; === bar 函数的机器码实际顺序 ===
stp x29, x30, [sp, #-48]!   ← ① 序言：SP 已下移，写入了内存
stp x22, x21, [sp, #0x10]   ←   后端生成，在 IR 检查之前
stp x20, x19, [sp, #0x20]

ldr x8, [tpidr_el0, ...]    ← ② IR Pass 插入的检查才在这里
sub x9, sp, #128
cmp x9, x8
b.lo __morestack

sub sp, sp, n               ← ③ VLA / 动态 alloca
```

①已经写入内存，②的检查还没运行——如果此时 SP 紧贴软边界，①的写操作就会越界。

**kFramePadding 通过"调用者的检查为被调函数的序言托底"来解决这个问题**，而不是靠函数自身的检查保护自身的序言。

逐步推导：

```
foo 的检查通过时，保证了：
    post_prologue_SP_foo - 128 >= bound

foo 完成自己的 VLA 后（若有，每次 VLA 也有相同的保证）：
    sp_foo_bottom 距 bound 至少还有 128B

foo 调用 bar（bl 不移动 SP）：
    bar 入口的 SP = sp_foo_bottom，距 bound 至少 128B

bar 的序言执行（最多写 96B）：
    最低写到 sp_foo_bottom - 96B
    由于 96B < 128B，这些写操作全部落在 bound 以上 ✅

bar 的 IR 检查执行：
    序言已内含于实际 SP，检查结果正确 ✅
```

用内存布局来表达：

```
foo 检查通过后：

  │  foo 的帧（alloca 已完成）          │
  │  ← sp_foo_bottom                   │
  │                                    │
  │  ┌──────────────────────────────┐  │
  │  │  ≥ 128B 缓冲区               │  │
  │  │  bar 序言写在这里（≤ 96B）   │  │ ← ① 安全
  │  │  bar 的 IR 检查在这里执行    │  │ ← ② 才开始检查
  │  └──────────────────────────────┘  │
  │                                    │
低地址  ─ ─ ─ ─ ─ ─ 软边界 ─ ─ ─ ─ ─ ─ ─
```

这个保证在每一层调用上都成立，形成**滚动式保护**：

```
foo 检查通过 → foo VLA 安全 + bar 序言安全
bar 检查通过 → bar VLA 安全 + baz 序言安全
baz 检查通过 → ...
```

每一层检查都为"自己之后、下一层检查之前"这段无保护窗口托底。

因此 **kFramePadding 必须 ≥ AArch64 最大序言大小（96 B）**，当前 128 B 满足这个约束，同时也为 `-O0` 或开启 `-fstack-protector` 留了余量。
### C. `__morestack` 的设计空间

当**入口检查**失败时，有两种不同的处置策略：

**策略 A（当前实现）——连续栈 + abort**

```
检查失败时：
  1. 切换 SP 到静态紧急栈（64 KiB 缓冲区顶部）
  2. 调用 __morestack_overflow（打印诊断信息，abort）
```

优点：实现简单（约 15 条指令），标准 DWARF unwinder 无需修改，溢出一定被检测到。  
代价：不支持动态扩展，溢出即终止。适合"已知上界"的场景（嵌入式守护进程、后台线程）。

**策略 B（可选，Hopter 分段栈）——Entry-restart**

函数尚未执行函数体代码，可以安全地在新栈片上重新启动：

```
检查失败时：
  1. 分配大小为（frame_estimate + 额外量）的新栈片
  2. 保存原始参数（x0–x7，若有 sret 则包括 x8）
  3. 将 SP 切换到新栈片的顶端
  4. 恢复原始参数
  5. 跳转到函数入口点（不是 ss.cont——从头重新执行）
  6. 函数的 ret 执行时：
       a. 将 SP 切回旧栈片
       b. 释放新栈片
       c. 返回原始调用点
```

步骤 5 之后，函数在新栈片内从头到尾完整运行——"一函数一栈片"不变量得到保证。  
代价：需要解决 VLA 与 unwind 协调问题（见局限性章节）。

当前演示采用策略 A。策略 B 的入口重启（步骤 1–6）在本仓库中**未实现**，仅记录于此供参考。

### D. IR Pass 能力边界总结

| 需求 | IR Pass | 后端补丁 |
|---|---|---|
| 在函数入口插入栈检查 | ✅ | ✅ |
| 获取精确帧大小 | ✅ 直接读取序言后的实际 SP | ✅ `MFI.getStackSize()` |
| 精确覆盖静态 alloca | ✅ 后端序言已含 S+C+P+K+G | ✅ 含于 MFI |
| 覆盖动态 VLA（逐 alloca 检查） | ✅ 在每个 VLA 后插入检查 | ✅ 后端自然插入 |
| 溢出检测并 abort | ✅ 紧急栈 + 诊断 | ✅ 可在运行时实现 |
| 标准 DWARF unwind 兼容 | ✅ 连续栈，无需修改 | ✅ |
| Entry-restart（分段栈扩展） | ❌ 当前未实现 | ✅ 可在运行时实现 |
| 修改编译器 | ❌ 不需要 | ✅ 必须 |

IR Pass 是栈检查**检测**机制的完整且正确的实现，有两个刻意的取舍：

1. 对后端不可见的帧层组成部分（C+P+K+G）不需要估算——依赖序言已完成的实际 SP。
2. 对帧大小大于 kFramePadding 的 VLA 触发 abort → 嵌入式目标上的明确大小契约。
3. 无需修改编译器，标准工具链即可使用。

---

## 参考资料

- LLVM 新 PassManager 插件文档：https://llvm.org/docs/WritingAnLLVMNewPMPass.html
- GCC split-stack ABI（对比参考）：https://gcc.gnu.org/wiki/SplitStacks
- Hopter Patch 1 背景：`obsi/hopter/hopter-toolchain-implementation.md` §2 / §10.7
