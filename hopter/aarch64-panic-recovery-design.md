# AArch64 no_std 内核：Panic 恢复与栈保护系统设计

本文档描述在 AArch64 bare-metal no_std 内核中实现 Hopter 风格"panic 恢复 + 强制栈保护"的完整系统设计。

设计目标：
- 用户任务代码只写 `panic!()`，不感知任何恢复机制
- panic 后自动执行 Drop cleanup，资源正确释放
- 任务 panic 后内核可重启该任务，系统继续运行
- 栈溢出被提前检测，同样走 unwind 路径恢复，不崩溃
- 无需修改 rustc/LLVM，只依赖树外 LLVM IR Pass 插件

---

## 一、总体架构

```
┌─────────────────────────────────────────────────────────────────┐
│                         用户任务代码                              │
│   level_a() → level_b() → level_c() → panic!() / 栈溢出        │
│   （只写业务逻辑，不感知任何恢复机制）                             │
└────────────────────────┬────────────────────────────────────────┘
                         │ 触发 panic 或栈溢出
                         ▼
┌────────────────────────────────────────────────────────────────┐
│                       Panic 入口层                              │
│                                                                 │
│  路径 A（用户主动 panic!）:                                      │
│    panic_handler → begin_panic(Box::new(()))                   │
│                                                                 │
│  路径 B（栈溢出强制 panic）:                                     │
│    overflow check → __morestack → [修改 LR] → ret              │
│    → overflow_panic_entry → _Unwind_RaiseException(静态 exc)   │
└────────────────────────┬────────────────────────────────────────┘
                         │ _Unwind_RaiseException
                         ▼
┌────────────────────────────────────────────────────────────────┐
│                    unwinding crate（已有库）                     │
│                                                                 │
│  __wrap__Unwind_RaiseException（内核钩子，调用一次）:            │
│    double panic 检测 / 降低任务优先级 / 可选并发重启             │
│                                                                 │
│  Phase 1（search）: 遍历 .eh_frame，找 catch_unwind landing pad │
│  Phase 2（cleanup）: 逐帧执行 Drop cleanup                      │
│                                                                 │
│  __wrap__Unwind_Resume（内核钩子，每 cleanup 帧后调用一次）:     │
│    看门狗喂狗 / 调度器 yield / 统计                             │
└────────────────────────┬────────────────────────────────────────┘
                         │ 跳入 catch_unwind landing pad
                         ▼
┌────────────────────────────────────────────────────────────────┐
│                   task_run（OS 框架层 / 线程入口）               │
│                                                                 │
│  catch_unwind 接住展开结果                                       │
│  Ok  → 任务正常退出，当前线程终止                                │
│  Err → 展开完成，spawn 新线程重启任务，当前线程终止              │
└────────────────────────┬────────────────────────────────────────┘
                         │ kernel_spawn_thread
                         ▼
┌────────────────────────────────────────────────────────────────┐
│                       内核调度器                                 │
│  分配新栈、新 TCB，放入就绪队列                                   │
│  回收旧任务的栈和 TCB                                            │
└────────────────────────────────────────────────────────────────┘
```

---

## 二、编译期：栈检查插桩（LLVM IR Pass 插件）

### 2.1 插件来源

`aarch64-split-stack-plugin`：树外 LLVM IR Pass，编译为 `SplitStackPass.so`，在
clang 编译管线中作为 `-fpass-plugin` 加载。无需修改 rustc/LLVM 源码。

### 2.2 插桩逻辑

Pass 在每个函数的入口块（IR 层面）插入栈边界检查。后端代码生成时，函数 prologue
已经把静态帧空间写入 SP（`sub sp, sp, #N`），因此 Pass 在 IR 层读取的 SP 值
已反映真实剩余空间。

**插桩后生成的汇编（示意）**：
```asm
function_entry:
    stp  x29, x30, [sp, #-N]!      ← 后端 prologue（已有，不变）
    sub  sp, sp, #rest_of_frame

    // ── Pass 插入的检查 ──
    ldr  x9, [x28, #BOUND_TLS_OFFSET]  // 读 TLS 中的 __hopter_stklet_bound
    cmp  sp, x9
    b.lo __morestack                    // 溢出时跳转
    // ── 检查结束，正常函数体继续 ──
    ...
```

**对 VLA（动态 alloca）的处理**：每次 `alloca` 之后立即插入同样的检查，防止动态
分配绕过入口检查。

### 2.3 关键参数：`kFramePadding` 与 `kGuardMargin`

两个参数各司其职：

```
实际栈底
  │
  ├── + kGuardMargin ──→ __hopter_stklet_bound（软边界）
  │                            │
  │   ← 这段空间供             │ 检查触发点：
  │     overflow 处理路径      │ SP - frame_estimate < bound
  │     （panic/unwind）使用   │
  │
  ├── 检查触发时的实际 SP（约在 bound 附近，由 kFramePadding 控制余量）
  │
  └── SP（prologue 执行后）
```

- **`kFramePadding`（128 B）**：插桩时帧大小的过估量，覆盖 callee-save 区域。检查
  比真实溢出早触发约 128 B，无 UB 风险，只是略保守。
- **`kGuardMargin`**：软边界到实际栈底之间的保留空间，供溢出处理路径（`overflow_panic_entry`
  → `begin_panic` → `_Unwind_RaiseException`）执行使用。**这是决定 per-task
  静态内存开销的关键参数**，见下节。

### 2.4 溢出处理的两阶段实现策略

溢出发生时，处理路径需要足够的栈空间执行 `panic + unwind`，主要开销约 1 KB。
有两种方式提供这块空间，可分阶段实现：

#### 阶段一（早期验证）：静态预留 `kGuardMargin`

`overflow_panic_entry` 直接在任务栈的 guard 区域内运行。实现最简单，无需任何
内核内存管理接入：

```
任务栈（已分配）
  [正常帧区域]     ← kGuardMargin 之上，正常使用
─────────────────  ← __hopter_stklet_bound（软边界）
  [guard 区域]    ← overflow_panic_entry / begin_panic / unwinder 在此运行
─────────────────  ← 实际栈底（硬边界）
```

`kGuardMargin` 必须 ≥ 2 KB（覆盖 panic + unwind 全路径）。per-task 静态开销 2 KB，
适合早期验证阶段使用。

#### 阶段二（优化）：`__morestack` 动态扩页

`kGuardMargin` 缩小到仅供 `__morestack` 自身运行（~512 B），溢出时动态向下映射
一个新物理页（4 KB）作为扩展栈：

```
任务栈（原有，虚地址连续）
  [正常帧区域]
─────────────────  ← 原 __hopter_stklet_bound
  [512B guard]    ← 仅供 __morestack + kernel_extend_task_stack 执行
─────────────────  ← 原实际栈底
  [扩展页 4KB]    ← __morestack 动态映射，overflow_panic_entry / unwind 在此运行
─────────────────  ← 新实际栈底
```

因为虚地址连续，DWARF unwinder 对扩展完全透明，无需任何 CFI 改动。内核只需提供
一个函数：

```rust
/// 在当前任务栈底部向下映射一个新页，更新 TCB 和 TLS bound，返回新页顶部地址。
/// 必须在 ~512B 栈空间内完成（__morestack 的 guard 余量）。
fn kernel_extend_task_stack() -> *mut u8;
```

`__morestack` 的实现变为：

```asm
__morestack:
    stp  x29, x30, [sp, #-16]!      // 自身帧，压在 512B guard 里
    mov  x29, sp

    bl   kernel_extend_task_stack   // 分配页 + 映射 + 更新 bound，返回新 SP

    mov  sp, x0                     // 切换到扩展页
    adr  x30, overflow_panic_entry  // 重定向 LR
    ldp  x29, xzr, [x29]
    add  sp, sp, #16

    ret   // SP 在扩展页，栈虚地址连续，DWARF 正常工作
```

任务回收时扩展页随 TCB 一并释放，无内存泄漏。

**两阶段对比**：

| | 阶段一（静态预留） | 阶段二（动态扩页） |
|---|---|---|
| kGuardMargin | 2 KB | 512 B |
| 溢出时额外分配 | 无 | 4 KB（按需，回收后释放） |
| per-task 常驻开销 | 2 KB | 512 B |
| 内核接入 | 无 | `kernel_extend_task_stack` |
| CFI 改动 | 无 | 无 |
| 适用阶段 | 早期验证 | 生产内核 |

### 2.4 每任务 TLS 变量

```c
// __hopter_stklet_bound：每任务独立，存储该任务栈底地址
__thread uintptr_t __hopter_stklet_bound;
```

内核创建任务时初始化：
```rust
// 设置 TLS 中的栈边界（任务栈底 + 安全区）
write_tls(BOUND_OFFSET, task_stack_bottom + SAFETY_MARGIN);
```

### 2.5 构建集成（Rust 项目）

```toml
# .cargo/config.toml
[build]
rustflags = [
    "-Cllvm-args=-load-pass-plugin=/path/to/SplitStackPass.so",
    "-Cllvm-args=-passes=split-stack",
]
```

---

## 三、运行时：`__morestack`（内核提供，替换插件默认实现）

### 3.1 设计原则

插件对外契约只有一条："溢出时 `bl __morestack`"。具体 `__morestack` 做什么由
链接进来的 runtime 决定。内核提供自己的版本，覆盖插件自带的 `abort` 版本。

**核心思路**：不从 `__morestack` 发起 panic（此时可能在紧急栈上），而是修改保存
的 LR，让 `ret` 后 CPU 回到任务栈，在任务栈上执行 `overflow_panic_entry`。

### 3.2 汇编实现

```asm
// kernel/src/asm/morestack.S
.global __morestack
.type   __morestack, @function
__morestack:
    // 进入时：SP 在任务栈上，kFramePadding 的余量仍可用
    // LR（x30）= 溢出函数中 bl __morestack 之后的指令地址

    // 1. 保存通用寄存器（使用任务栈上的余量，不切紧急栈）
    stp  x0,  x1,  [sp, #-16]!
    stp  x2,  x3,  [sp, #-16]!
    stp  x4,  x5,  [sp, #-16]!
    stp  x6,  x7,  [sp, #-16]!
    stp  x29, x30, [sp, #-16]!    // 保存 FP 和 LR

    // 2. 将保存的 LR 覆盖为 overflow_panic_entry
    //    ret 后 CPU 将跳到 overflow_panic_entry，仍在任务栈上
    adr  x9, overflow_panic_entry
    str  x9, [sp, #0]             // 覆盖刚保存的 LR（栈顶偏移 0）

    // 3. 恢复寄存器（新 LR 已生效）
    ldp  x29, x30, [sp], #16
    ldp  x6,  x7,  [sp], #16
    ldp  x4,  x5,  [sp], #16
    ldp  x2,  x3,  [sp], #16
    ldp  x0,  x1,  [sp], #16

    // 4. ret：返回地址是 overflow_panic_entry，在任务栈上执行
    ret
```

**执行效果**：`__morestack` 返回后，CPU 的 SP、FP、所有通用寄存器均恢复原状，
下一条执行的指令是 `overflow_panic_entry`，任务栈完整可用。

### 3.3 为什么不切换到紧急栈再发起 panic

如果在紧急栈上调用 `begin_panic`，unwinder 会从紧急栈帧开始向上遍历，永远找不到
任务栈上的 `catch_unwind` landing pad（`task_run` 的帧在任务栈上）。紧急栈可以
保留用于其他目的，但不能作为展开的起点。

---

## 四、两条 Panic 路径

### 4.1 路径 A：用户主动 `panic!`

`panic!()` 宏调用 Rust 编译器内置的 panic 基础设施，最终调用
`#[panic_handler]`。在 no_std 中，`panic_handler` 完全由内核实现，后续展开
需要手动发起。

```rust
// kernel/src/panic.rs

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    // 可选：记录 panic 位置到日志缓冲区
    log_panic_location(info);

    // 可选：通知调度器（降低当前任务优先级，让其他任务先运行）
    // 具体效果由 __wrap__Unwind_RaiseException 中实现

    // 启动展开：begin_panic 内部调用 _Unwind_RaiseException
    // 走 __wrap__Unwind_RaiseException → Phase1 → Phase2 → catch_unwind
    let _ = unwinding::panic::begin_panic(Box::new(()));

    // 不可达：begin_panic 永远跳走，不会返回
    loop { core::hint::spin_loop(); }
}
```

**`begin_panic` 做的事**：
1. `Box::new(payload)` 在堆上分配 `UnwindException`（约 64 字节）
2. 初始化 `exception_class`（标识为 Rust panic）、`exception_cleanup` 回调
3. 调用 `_Unwind_RaiseException(exception_ptr)` 发起两遍遍历

### 4.2 路径 B：栈溢出强制 panic

栈溢出时不能用 `begin_panic`（可能没有足够栈空间做 `Box::new`，且 `begin_panic`
自身也有栈帧开销）。使用预分配的 `UnwindException` + 裸函数尾调绕过堆分配。

```rust
// kernel/src/unwind.rs

// 每个任务的 TCB 中预分配一个 UnwindException
// 任务创建时初始化，panic 时直接使用，无需堆分配
pub struct TaskUnwindStorage {
    pub exception: UnsafeCell<UnwindException>,
    pub in_use: AtomicBool,
}

// overflow_panic_entry：裸函数，零栈帧，尾调 _Unwind_RaiseException
#[naked]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn overflow_panic_entry() -> ! {
    core::arch::naked_asm!(
        // 1. 从 TLS 读取当前任务 TCB 指针（内核约定：TPIDR_EL1 存 TCB 地址）
        "mrs  x0, tpidr_el1",
        // 2. 取 TCB 中预分配的 UnwindException 地址
        "ldr  x0, [x0, #{exc_offset}]",
        // 3. 初始化 exception_class（8 字节，"RUSTPANIC" 类标识）
        "mov  x1, #0x5255535450414E49",   // "RUSTPANI"（little-endian）
        "str  x1, [x0, #0]",
        // 4. 清零 private 字段（unwinder 要求）
        "stp  xzr, xzr, [x0, #8]",
        "stp  xzr, xzr, [x0, #24]",
        // 5. 尾调 _Unwind_RaiseException，不压任何栈帧
        "b    _Unwind_RaiseException",
        exc_offset = const TASK_UNWIND_EXC_OFFSET,
    )
}
```

**为什么安全**：
- 裸函数本身不压栈帧，x0-x7 是参数寄存器不需要保存
- `_Unwind_RaiseException` 尾调后使用的是函数自身的栈帧（从当前 SP 开始向下）
- 此时 SP 距边界有 `kFramePadding` 的余量（≥ 4 KB），足够 unwinder 运行

---

## 五、内核感知钩子：`--wrap` 机制

在展开路径的两个关键节点插入内核逻辑，无需修改 `unwinding` crate。

### 5.1 配置

```toml
# .cargo/config.toml
[build]
rustflags = [
    "-Clink-arg=-Wl,--wrap=_Unwind_RaiseException",
    "-Clink-arg=-Wl,--wrap=_Unwind_Resume",
]
```

链接器将所有对 `_Unwind_RaiseException` 的调用重写为 `__wrap__Unwind_RaiseException`，
原实现重命名为 `__real__Unwind_RaiseException`。

### 5.2 `__wrap__Unwind_RaiseException`（展开开始，全局调用一次）

```rust
// kernel/src/unwind_hooks.rs

unsafe extern "C-unwind" {
    fn __real__Unwind_RaiseException(exc: *mut UnwindException) -> UnwindReasonCode;
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn __wrap__Unwind_RaiseException(
    exc: *mut UnwindException,
) -> UnwindReasonCode {
    // 1. Double panic 检测
    //    如果当前任务已经在展开中（TASK_UNWINDING == true），说明 Drop 里再次 panic
    //    这种情况无法安全恢复，直接终止任务
    if TASK_UNWINDING.swap(true, Ordering::SeqCst) {
        // double panic：跳过 unwind，直接销毁任务
        kernel_terminate_current_task();
        loop {}
    }

    // 2. 降低任务优先级
    //    展开期间 CPU 密集，让其他就绪任务有机会运行
    //    展开结束后在 task_run 的 Err 分支恢复原优先级
    scheduler_lower_task_priority(current_task_id(), UNWIND_PRIORITY);

    // 3. 可选：并发重启（在展开开始时就 spawn 替换任务）
    //    如果任务标记为可重启，提前创建新任务，并发执行
    //    注意：新任务与旧任务的共享资源需要在任务设计层面处理好
    // if current_task_is_restartable() {
    //     kernel_spawn_thread(task_run, current_task_fn());
    // }

    unsafe { __real__Unwind_RaiseException(exc) }
}
```

### 5.3 `__wrap__Unwind_Resume`（每个 cleanup 帧后调用，调用 N 次）

```rust
// kernel/src/unwind_hooks.rs

unsafe extern "C-unwind" {
    fn __real__Unwind_Resume(exc: *mut UnwindException) -> !;
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn __wrap__Unwind_Resume(
    exc: *mut UnwindException,
) -> ! {
    // 喂看门狗：防止展开帧数过多导致看门狗超时重启
    watchdog_feed();

    // 调度器 yield 点：让高优先级任务有机会抢占
    // （展开期间任务优先级已降低，yield 后可能切换到其他任务）
    scheduler_yield_if_higher_priority_ready();

    // 可选：统计 cleanup 帧数，用于调试和性能分析
    UNWIND_FRAME_COUNT.fetch_add(1, Ordering::Relaxed);

    unsafe { __real__Unwind_Resume(exc) }
}
```

**关键区别**：

| 钩子 | 调用次数 | 适合做的事 |
|---|---|---|
| `__wrap__Unwind_RaiseException` | 1 次（展开开始） | double panic 检测、降优先级、并发重启 |
| `__wrap__Unwind_Resume` | N 次（每 cleanup 帧后） | 看门狗、yield、统计 |

### 5.4 ISR 中的 Panic 路径

IRQ 处理函数（ISR）中 `begin_panic` 的 `Box::new` 可能触发重入分配器。使用
全局静态预分配绕过堆分配。

```rust
// kernel/src/panic.rs

// 全局唯一 ISR 专用 exception（IRQ 上下文单核执行，一个就够）
static ISR_UNWIND_EXC: UnsafeCell<MaybeUninit<UnwindException>> =
    UnsafeCell::new(MaybeUninit::uninit());
static ISR_EXC_IN_USE: AtomicBool = AtomicBool::new(false);

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    log_panic_location(info);

    if is_in_isr() {
        // ISR 路径：使用静态预分配
        if ISR_EXC_IN_USE.swap(true, Ordering::SeqCst) {
            // ISR 中 double panic，无法安全恢复，halt
            loop { core::hint::spin_loop(); }
        }
        unsafe {
            let exc = (*ISR_UNWIND_EXC.get()).as_mut_ptr();
            init_unwind_exception(exc);
            _Unwind_RaiseException(exc);
        }
    } else {
        // 普通任务路径
        let _ = unwinding::panic::begin_panic(Box::new(()));
    }
    loop {}
}
```

---

## 六、任务框架：`task_run`

`task_run` 是线程的入口函数，每个任务线程都以它为入口。它提供 `catch_unwind`
landing pad，并在展开结束后处理任务的生命周期。

```rust
// kernel/src/task.rs

/// 任务线程入口函数。
/// 内核创建线程时传入：kernel_create_thread(task_run, task_fn)
pub fn task_run(task_fn: fn()) {
    // 初始化 per-task 展开状态
    TASK_UNWINDING.store(false, Ordering::SeqCst);

    // 初始化预分配的 UnwindException（供栈溢出路径使用）
    unsafe { init_task_overflow_exception(current_tcb()); }

    // 注册 landing pad，运行用户函数
    // catch_unwind 在 .eh_frame 中生成 landing pad 信息
    // unwinder 通过 .eh_frame 找到这里，展开结束后跳回此处
    let result = unwinding::panic::catch_unwind(task_fn);

    match result {
        Ok(_) => {
            // 用户函数正常返回，任务完成
            // task_run 返回 → 内核调度器回收当前线程
        }

        Err(_) => {
            // 展开完成：task_fn 调用链上所有 Drop 已执行，资源全部释放

            // 恢复任务优先级（__wrap__Unwind_RaiseException 中降低的）
            scheduler_restore_priority(current_task_id());

            // 清理展开标志
            TASK_UNWINDING.store(false, Ordering::SeqCst);

            // 清理溢出 exception 的 in_use 标志
            unsafe { reset_task_overflow_exception(current_tcb()); }

            // 根据任务策略决定是否重启
            if current_task_is_restartable() {
                // 创建一个新线程重新执行同一任务函数
                // 新线程有全新的栈和 TCB，与旧线程完全独立
                kernel_spawn_thread(task_run, task_fn);
            }

            // 当前线程退出，调度器回收栈和 TCB
            // （task_run 函数返回 → 内核线程退出流程）
        }
    }
}
```

### 6.1 线程与任务的关系

```
内核线程（OS 原语）  ← 管理：栈内存、TCB、调度队列
     └── task_run   ← 框架：提供 landing pad、处理生命周期
          └── task_fn ← 用户：只写业务逻辑
```

一个线程对应一个 `task_run` 执行周期。panic 后 `task_run` 返回，线程结束，内核
可以创建一个全新的线程（新栈、新 TCB）来重启相同的 `task_fn`。

### 6.2 串行重启 vs. 并发重启

| 策略 | 时机 | 实现位置 | 复杂度 |
|---|---|---|---|
| 串行重启 | 展开完成后 spawn | `task_run` 的 `Err` 分支 | 低 |
| 并发重启 | 展开开始时 spawn | `__wrap__Unwind_RaiseException` | 中（需处理资源隔离） |

推荐先实现串行重启。并发重启的时延优势在大多数嵌入式场景中意义不大，而资源
隔离问题（旧任务还在展开时新任务已经访问共享资源）增加了设计复杂度。

---

## 七、`unwinding` crate 的配置

```toml
# Cargo.toml
[dependencies]
unwinding = { version = "0.2.8", default-features = false, features = [
    "unwinder",       # 提供 _Unwind_RaiseException, _Unwind_Resume 等核心实现
    "fde-static",     # 静态链接的 .eh_frame，不依赖动态加载（bare-metal 必须）
    "personality",    # 提供 rust_eh_personality（personality routine）
    "panic",          # 提供 begin_panic, catch_unwind
] }
```

**`fde-static` 的作用**：指示 unwinder 通过 `__eh_frame_start`/`__eh_frame_end`
符号（链接器脚本定义）定位 `.eh_frame` 段，不依赖动态链接器提供的信息。
bare-metal 必须使用此 feature。

**链接脚本要求**：
```ld
/* memory.x 或 link.ld */
SECTIONS {
    .eh_frame : {
        __eh_frame_start = .;
        KEEP(*(.eh_frame .eh_frame.*))
        __eh_frame_end = .;
    }
}
```

---

## 八、各组件实现清单

| 组件 | 文件位置 | 实现工作量 | 备注 |
|---|---|---|---|
| LLVM IR Pass 插桩 | `SplitStackPass.so`（已有） | 0 | 插件已实现 |
| `__hopter_stklet_bound` TLS 初始化 | `kernel/src/task.rs` | 小 | 任务创建时设置 |
| `__morestack` 汇编 | `kernel/src/asm/morestack.S` | 小（~30 行汇编） | 修改 LR 后 ret |
| `overflow_panic_entry` 裸函数 | `kernel/src/unwind.rs` | 小（~15 行汇编） | 尾调 `_Unwind_RaiseException` |
| `panic_handler` | `kernel/src/panic.rs` | 小（~20 行） | 调用 `begin_panic` 或静态路径 |
| `__wrap__Unwind_RaiseException` | `kernel/src/unwind_hooks.rs` | 小（~20 行） | double panic + 优先级 |
| `__wrap__Unwind_Resume` | `kernel/src/unwind_hooks.rs` | 小（~10 行） | 看门狗 + yield |
| `task_run` 框架 | `kernel/src/task.rs` | 小（~30 行） | catch_unwind + 重启逻辑 |
| ISR 静态 exception | `kernel/src/panic.rs` | 小（~20 行） | 可选，ISR 场景需要 |
| 预分配 overflow exception | `kernel/src/task.rs` | 小（~20 行） | 每任务 TCB 中 |

**不需要实现**（由 `unwinding` crate 完全承担）：
- DWARF `.eh_frame` 解析与帧步进
- LSDA（Language Specific Data Area）解析
- landing pad 地址查找
- 寄存器状态恢复（CFA 计算）
- personality routine（`rust_eh_personality`）

---

## 九、两条 Panic 路径的完整执行流

### 路径 A 执行流

```
用户代码 panic!("msg")
  │
  ├─ Rust 编译器调用 #[panic_handler]
  │
  ▼
panic_handler(info)
  ├─ log_panic_location(info)       // 可选：记录位置
  └─ begin_panic(Box::new(()))      // 启动展开
       │
       ├─ Box::new：堆分配 UnwindException（~64 字节）
       └─ _Unwind_RaiseException(exc_ptr)
            │
            ▼ （被 --wrap 拦截）
       __wrap__Unwind_RaiseException(exc_ptr)
            ├─ double panic 检测（TASK_UNWINDING swap）
            ├─ scheduler_lower_task_priority()
            └─ __real__Unwind_RaiseException(exc_ptr)
                 │
                 ├─ Phase 1（search）: 遍历 .eh_frame
                 │    level_c → level_b → level_a → task_run
                 │    在 task_run 帧找到 catch_unwind landing pad，停止
                 │
                 └─ Phase 2（cleanup）: 从 level_c 重新向上
                      │
                      ├─ level_c cleanup landing pad
                      │    执行 Resource("C::file_handle")::drop()
                      │    bl _Unwind_Resume → __wrap__Unwind_Resume
                      │         └─ watchdog_feed() + __real__Unwind_Resume
                      │
                      ├─ level_b cleanup landing pad
                      │    执行 Resource("B::connection")::drop()
                      │    bl _Unwind_Resume → __wrap__Unwind_Resume
                      │
                      ├─ level_a cleanup landing pad
                      │    执行 Resource("A::mutex_guard")::drop()
                      │    执行 Resource("A::vec_data")::drop()
                      │    bl _Unwind_Resume → __wrap__Unwind_Resume
                      │
                      └─ 跳入 task_run 的 catch_unwind landing pad
                           │
                           ▼
                      task_run: result = Err(_)
                           ├─ scheduler_restore_priority()
                           ├─ TASK_UNWINDING = false
                           └─ kernel_spawn_thread(task_run, task_fn)  // 重启
                                │
                                ▼
                           当前线程退出，新线程加入就绪队列
```

### 路径 B 执行流

```
level_c() 函数入口：SP 已低于 __hopter_stklet_bound
  │
  ├─ 插桩代码：cmp sp, x9 → b.lo __morestack
  │
  ▼
__morestack（汇编）
  ├─ 保存寄存器（在 kFramePadding 余量上）
  ├─ 覆盖保存的 LR → overflow_panic_entry
  ├─ 恢复寄存器
  └─ ret → 跳到 overflow_panic_entry，仍在任务栈上
       │
       ▼
overflow_panic_entry（裸函数）
  ├─ 读 TLS → TCB → 预分配的 UnwindException
  ├─ 初始化 exception_class
  └─ b _Unwind_RaiseException（尾调，零栈帧开销）
       │
       ▼ （后续与路径 A 完全相同）
  __wrap__Unwind_RaiseException → Phase1 → Phase2 → task_run
```

---

## 十、设计约束与注意事项

### 10.1 `kFramePadding` 与 `kGuardMargin` 的选取

**`kFramePadding`（固定 128 B）**：覆盖 callee-save 区域（AArch64 最多 96 B）加对齐
余量，128 B 已足够，无需调整。

**`kGuardMargin`**：取决于实现阶段：
- 阶段一（静态预留）：2 KB，覆盖 `overflow_panic_entry` + `begin_panic` + unwinder 全路径
- 阶段二（动态扩页）：512 B，仅覆盖 `__morestack` + `kernel_extend_task_stack` 自身执行

### 10.2 分配器重入

`begin_panic` 的 `Box::new` 在任务上下文中安全，因为任务不会被中断打断分配器。
ISR 上下文中必须使用静态预分配路径。

### 10.3 Drop 中再次 panic（Double Panic）

`__wrap__Unwind_RaiseException` 通过 `TASK_UNWINDING` 标志检测。发生时终止当前
任务，不能触发递归展开。此标志是 per-task 的（存储在 TCB 或 TLS 中），多任务
并发展开互不干扰。

### 10.4 `overflow_panic_entry` 中的寄存器状态

`__morestack` 返回后，x0-x18 等调用者保存寄存器的值对调用方（溢出函数）仍然有效。
`overflow_panic_entry` 用 x0 传参给 `_Unwind_RaiseException`，会覆盖 x0，但此时
已不会返回到溢出函数，覆盖无害。

### 10.5 `.eh_frame` 中 `overflow_panic_entry` 的帧信息

`overflow_panic_entry` 是裸函数，编译器不生成其 `.eh_frame` 条目，unwinder 无法
"展开进入"它。这是期望行为：展开从 `overflow_panic_entry` 调用 `_Unwind_RaiseException`
时，unwinder 的起始上下文就是当时的 SP（即任务栈，`__morestack` 返回后的状态），
能正确向上找到 `level_c`、`level_b`、`level_a`、`task_run` 的帧。

---

## 十一、与 Hopter 设计的对应关系

| Hopter 组件 | 本设计对应 | 差异说明 |
|---|---|---|
| 定制 LLVM 编译器（栈检查插桩） | `SplitStackPass.so`（LLVM IR Pass 插件） | 无需修改编译器源码 |
| `start_unwind_entry`（~80 行裸汇编） | `panic_handler` → `begin_panic`（5 行） | AArch64 连续栈不需要保存现场/切换栈 |
| `__morestack`（分段栈分配） | `__morestack`（修改 LR，deferred unwind） | 不分配新 stacklet，直接触发展开 |
| ARM EHABI 解析（`unw_table.rs`） | `unwinding` crate 原生支持 `.eh_frame` | 无需实现，格式不同语义相同 |
| LSDA 解析（`unw_lsda.rs`） | `unwinding` crate 内置 personality routine | 无需实现 |
| `UnwindState::step()`（帧步进） | `unwinding` crate 内部实现 | 无需实现 |
| Landing pad 跳转（SVC + exception return） | `unwinding` 直接 longjmp 到 landing pad | 连续栈不需要 SVC，无特权级切换 |
| `unw_catch.rs`（custom catch_unwind） | `unwinding::panic::catch_unwind` | 直接使用 crate 提供的实现 |
| Double panic 检测（`is_unwinding`） | `__wrap__Unwind_RaiseException` 中 AtomicBool | 语义相同，实现更简洁 |
| 任务优先级降低（`create_unwind_state`） | `__wrap__Unwind_RaiseException` 中调度器调用 | 语义相同 |
| 并发重启（`try_concurrent_restart`） | `task_run` Err 分支 spawn 新线程 | 改为串行重启，复杂度更低 |
| 专用 unwinder stacklet（`TaskUnwindPrepare`） | 不需要 | 连续栈，展开器在任务自身栈上运行 |
| ISR static UnwindState | 静态预分配 `UnwindException` | 语义相同，实现略有不同 |
| `forced.rs`（栈溢出强制展开） | `__morestack` + `overflow_panic_entry` | 实现方式完全不同，但语义等价 |
