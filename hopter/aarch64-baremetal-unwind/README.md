# aarch64-baremetal-unwind

最小 no_std AArch64 bare-metal Rust 示例，演示 Hopter 风格的 panic 恢复流程：

1. 用户代码仅触发 panic
2. panic handler 进入 start_unwind_entry
3. start_unwind_entry 启动 unwind
4. unwind 逐帧执行 Drop cleanup
5. 找到 OS 层预注册的 landing pad
6. 跳回恢复执行流，系统继续运行

另外，本 demo 还演示了如何在 no_std runtime 中自定义“resume_unwind 继续展开”步骤：

- 使用链接器 `--wrap=_Unwind_Resume` 拦截编译器自动插入的 `_Unwind_Resume` 调用
- 进入 `__wrap__Unwind_Resume`（自定义逻辑）后，再转发给 `__real__Unwind_Resume`
- 运行日志中会打印拦截次数，用于验证你的 runtime 确实接管了继续展开路径

这个设计的目标是让用户代码不直接调用 catch_unwind，而由 OS 基础设施统一接管 panic 恢复逻辑。

---

## 新设计概要

当前代码采用三层结构：

- 用户层：level_a/level_b/level_c，只写业务逻辑和 panic。
- 运行时入口层：panic_handler 和 start_unwind_entry，负责启动 unwind。
- OS 任务层：task_run，在这里注册 landing pad 并接收恢复后的执行流。

关键点：

- 用户函数没有显式 catch_unwind。
- catch_unwind 仅存在于 task_run，属于 OS 框架代码。
- panic 后由 panic_handler 自动进入 start_unwind_entry。
- start_unwind_entry 调用 begin_panic 后，unwind 根据 eh_frame metadata 自动查找 landing pad 并执行 cleanup。

---

## 与 Hopter 的对应关系

| 本 demo | Hopter 中的概念 |
|---|---|
| panic_handler | Rust panic handler |
| start_unwind_entry | start_unwind_entry |
| begin_panic 启动展开 | UnwindState::step 逐步展开 |
| eh_frame + LSDA 查找 landing pad | exidx/extab 查找 landing pad |
| task_run 内 landing pad | task catch block |
| Drop 自动执行 cleanup | cleanup routine 执行资源回收 |

说明：在 AArch64 上元数据格式是 eh_frame，在 Cortex-M 上是 exidx/extab，格式不同但语义一致。

---

## 自定义 resume_unwind（本示例新增）

代码位置：

- `src/main.rs` 中 `__wrap__Unwind_Resume`
- `.cargo/config.toml` 中 `--wrap=_Unwind_Resume`

工作方式：

1. cleanup landing pad 执行结束后，编译器会调用 `_Unwind_Resume`
2. 链接器将该调用重写到 `__wrap__Unwind_Resume`
3. 你可以在 wrapper 里做统计、日志、策略钩子
4. 最后调用 `__real__Unwind_Resume`，回到标准展开流程

这就是在不重写整套 unwinder 的前提下，对“resume_unwind 阶段”进行可控插桩的最小方案。

---

## 执行流

1. main 调用 task_run(level_a)
2. task_run 在 OS 层注册 landing pad 后运行用户任务
3. level_c 触发 panic
4. Rust 运行时自动进入 panic_handler
5. panic_handler 调用 start_unwind_entry
6. start_unwind_entry 调用 begin_panic 启动展开
7. unwind 逐帧执行 Drop
8. unwind 找到 task_run 的 landing pad
9. 执行流跳回 task_run 的恢复分支
10. main 继续执行，系统不崩溃

---

## 代码位置

- panic handler 和展开入口在 src/main.rs
- 任务恢复框架 task_run 在 src/main.rs
- 用户任务 level_a/level_b/level_c 在 src/main.rs
- 保留 unwind section 的链接脚本在 memory.x
- 构建目标配置在 .cargo/config.toml

---

## 运行方式

1. 构建

```sh
cargo build
```

2. QEMU 运行（脚本）

```sh
./run_qemu.sh
```

3. QEMU 运行（手动）

```sh
qemu-system-aarch64 \
  -M virt \
  -cpu cortex-a53 \
  -nographic \
  -kernel target/aarch64-unknown-none/debug/aarch64-baremetal-unwind \
  -monitor none \
  -serial stdio \
  -smp 1 \
  -m 128M
```

---

## 已验证的关键输出

已在 QEMU 上验证可见如下关键日志顺序：

1. level_c 触发 panic
2. panic_handler 调 start_unwind_entry
3. Drop 顺序为 C、B、A（深帧到浅帧）
4. task_run 打印 unwind landed here
5. main 打印继续运行

这证明了：

- panic 后确实进入了 start_unwind_entry
- unwind 过程中 cleanup landing pad 被执行
- 执行流确实回到了 OS 层恢复点

---

## 注意事项

- 本 demo 的 landing pad 由 task_run 内的 catch_unwind 提供，但它属于 OS 层，不暴露给用户任务代码。
- 如果要进一步接近 Hopter，可以将 task_run 抽象成任务调度器入口，并在恢复后做任务重启或销毁策略。

---

## 基于 unwinding crate 将 Hopter unwind 迁移到 AArch64 no_std 内核

本节回答：如果自己在 AArch64 bare-metal 上实现内核（no_std，不使用分段栈），Hopter 的 unwind 子系统中每一个组件应该如何对应地实现。

### 为什么 AArch64 比 Cortex-M 简单

Hopter 在 Cortex-M 上必须从头实现整套 unwinder，原因有三：

1. **元数据格式**：Cortex-M 使用 ARM EHABI（`.ARM.exidx`/`.ARM.extab`），`unwinding` crate 不支持，Hopter 自己写了解析器。
2. **分段栈 crossing**：每次帧步进都可能跨 stacklet，需要嵌入 `step()` 主循环，无法在通用展开器外部处理。
3. **Landing pad 跳转**：连续栈不在同一内存块，需要通过 SVC + exception return 将 CPU 引导过去。

AArch64 连续栈 + DWARF `.eh_frame` 的组合，三个问题全部消失，`unwinding` crate 可以直接承担核心工作。

### 各组件迁移对应表

| # | Hopter 组件 | 所在文件 | AArch64 实现方式 | 工作量 |
|---|---|---|---|---|
| 1 | ARM EHABI 元数据解析（`.ARM.exidx`/`.ARM.extab`） | `unw_table.rs` | `unwinding` crate 原生支持 `.eh_frame`，完全不需要 | ✅ 零工作 |
| 2 | LSDA 解析（call-site table → landing pad 地址） | `unw_lsda.rs` | personality routine 内置在 `unwinding` crate 里，自动处理 | ✅ 零工作 |
| 3 | 帧步进（`UnwindState::step()`，恢复寄存器逐帧向上） | `unwind.rs` | `unwinding` crate 的 `_Unwind_Step`，由 `begin_panic` 驱动 | ✅ 零工作 |
| 4 | Landing pad 跳转（SVC + TrapFrame + exception return） | `start_unwind_entry` 汇编 | 连续栈不需要 SVC，`unwinding` 直接 longjmp 到 landing pad | ✅ 零工作 |
| 5 | 展开入口（`start_unwind_entry` 裸汇编，保存现场） | `unwind.rs` | `panic_handler` → `begin_panic(Box::new(()))` | ✅ 5 行代码 |
| 6 | 任务 catch 边界（`catch_unwind`） | `unw_catch.rs` | `unwinding::panic::catch_unwind(task_fn)` | ✅ 直接用 |
| 7 | `_Unwind_Resume` 拦截（cleanup 结束后继续展开） | `unwind.rs` | `--wrap=_Unwind_Resume`，本 demo 已演示 | ✅ 已演示 |
| 8 | Double panic 检测（`is_unwinding` / `set_unwinding`） | `unwind.rs` | 每个任务维护一个 `AtomicBool unwinding_flag` | 🔧 小工作 |
| 9 | ISR 中 panic 的静态 UnwindState | `unwind.rs` | IRQ 路径预分配静态 `UnwindException`，panic 时走静态路径 | 🔧 小工作 |
| 10 | Panic 时降低任务优先级 + 触发重调度 | `unwind.rs` `create_unwind_state` | 在 `--wrap=_Unwind_RaiseException` 里调用调度器 | 🔧 小工作 |
| 11 | 可重启任务（`try_concurrent_restart`） | `unwind.rs` | `catch_unwind` 返回 `Err` 后，内核在 `task_run` 末尾重新 spawn | 🔧 小工作 |
| 12 | 专用 unwinder stacklet（`TaskUnwindPrepare` SVC） | `start_unwind_entry` 汇编 | 无需此机制（连续栈，展开器运行在任务自身栈上） | ✅ 不需要 |
| 13 | 强制展开（`forced.rs`，栈溢出时 divert + deferred unwind） | `forced.rs` | LLVM IR Pass 插件在每个函数入口插入栈边界检查，溢出时 `__morestack` 调用 `begin_panic` | 🔧 插件已有，接 panic 即可 |

### 各组件详细说明

#### 组件 5：展开入口

Hopter 的 `start_unwind_entry` 是约 80 行裸汇编，主要工作是：保存现场寄存器、通过 SVC 申请专用 stacklet、手动构造 `UnwindInitContext`。

AArch64 连续栈上等价实现只需：

```rust
#[panic_handler]
fn panic_handler(_info: &PanicInfo) -> ! {
    // 可选：通知内核（降优先级、记录日志）
    kernel_notify_panic();
    // 启动展开，等价于 Hopter 的 start_unwind_entry
    let _ = unwinding::panic::begin_panic(Box::new(()));
    loop {}
}
```

`begin_panic` 内部负责分配 `UnwindException`、调用 `_Unwind_RaiseException`、驱动两遍遍历（search phase + cleanup phase），完全替代了那 80 行汇编。

#### 组件 7 + 8 + 10：内核感知的钩子

通过链接器 `--wrap` 机制在展开路径的关键节点插入内核逻辑：

```rust
// .cargo/config.toml:
// rustflags = ["-Clink-arg=-Wl,--wrap=_Unwind_RaiseException",
//              "-Clink-arg=-Wl,--wrap=_Unwind_Resume"]

// 展开开始时：做 double panic 检测、降低任务优先级
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn __wrap__Unwind_RaiseException(
    exc: *mut UnwindException,
) -> UnwindReasonCode {
    if TASK_UNWINDING_FLAG.swap(true, Ordering::SeqCst) {
        loop {} // double panic，直接 halt
    }
    kernel_lower_task_priority();
    unsafe { __real__Unwind_RaiseException(exc) }
}

// 每次 cleanup 结束后继续展开：可插入日志、统计
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn __wrap__Unwind_Resume(exc: *mut UnwindException) -> ! {
    // 在这里插入内核逻辑
    unsafe { __real__Unwind_Resume(exc) }
}
```

#### 组件 9：ISR 中的 panic

`begin_panic` 内部调用 `Box::new(...)` 在堆上分配 `UnwindException`，在 IRQ 上下文中堆分配可能不安全（重入分配器）。解决方案是静态预分配：

```rust
static mut ISR_EXCEPTION_STORAGE: MaybeUninit<UnwindException> = MaybeUninit::uninit();
static ISR_EXCEPTION_IN_USE: AtomicBool = AtomicBool::new(false);

#[panic_handler]
fn panic_handler(_: &PanicInfo) -> ! {
    if is_in_isr() {
        if ISR_EXCEPTION_IN_USE.swap(true, Ordering::SeqCst) {
            loop {} // double panic in ISR
        }
        let exc_ptr = unsafe { ISR_EXCEPTION_STORAGE.as_mut_ptr() };
        // 手动初始化并调用 _Unwind_RaiseException
        unsafe { _Unwind_RaiseException(exc_ptr) };
    } else {
        let _ = unwinding::panic::begin_panic(Box::new(()));
    }
    loop {}
}
```

#### 组件 11：可重启任务

Hopter 的并发重启逻辑嵌在 `create_unwind_state` 里。AArch64 上等价做法放在 `task_run` 的恢复分支里：

```rust
fn task_run(task_fn: fn(), restartable: bool) {
    loop {
        let result = unwinding::panic::catch_unwind(task_fn);
        match result {
            Ok(_) => break,
            Err(_) => {
                if restartable {
                    continue; // 清理残余状态后重新运行
                } else {
                    break;    // 销毁任务
                }
            }
        }
    }
}
```

#### 组件 13：强制展开

Hopter 的强制展开依赖定制编译器在每个函数 prologue 里插入栈检测指令。AArch64 上通过树外 LLVM IR Pass 插件实现同等效果，无需修改 rustc/LLVM。

插件位于 `aarch64-split-stack-plugin`，工作方式：

1. Pass 在每个函数入口插入一次检查：`(SP - kFramePadding) >= __hopter_stklet_bound`
2. 对动态 alloca（VLA）也在分配后立即插入同样的检查
3. 检查失败时调用 `__morestack`，它先切换到静态紧急栈，再调用溢出处理函数
4. **当前**溢出处理函数调用 `abort()`；**接入 unwind 只需改为调用 `begin_panic`**

```c
// runtime/runtime.c 中的溢出处理函数，改为触发 panic 而非 abort
void __hopter_stack_overflow_handler(void) {
    // 原来：abort();
    // 改为：触发 panic，走 begin_panic → unwind → landing pad
    rust_begin_panic();  // FFI 调用 Rust 的 panic 入口
}
```

Pass 本身不感知 EH/unwind，不需要修改。唯一工作是在运行时把 `abort()` 换成 `begin_panic`。

### 实现代价总结

```
不需要实现（由 unwinding crate 全部承担）：
  ├── 元数据格式解析（EHABI parser）
  ├── 帧步进（step / continue_unwind）
  ├── LSDA 解析（call-site table）
  └── Landing pad 跳转

需要自己实现（总计约 100~150 行）：
  ├── panic_handler（5 行）
  ├── --wrap 钩子（double panic + 优先级 + 日志，~30 行）
  ├── ISR 静态预分配路径（~30 行，可选）
  └── task_run 恢复策略（~20 行）

需要较小工作（插件已实现检测层）：
  └── 强制展开：__morestack 改调 begin_panic（插件 pass 无需改动）
```
