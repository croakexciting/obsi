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
