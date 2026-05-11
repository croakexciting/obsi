### 我看了你的设计，直接将 task 包在了 catch_unwind 中，但是这样可以支持多线程的情况吗，你这样启动的似乎不是一个线程，假设我运行在一个微内核中，比如 seL4 上，我怎么对某个线程，甚至协程做 unwind 呢

你这个问题非常关键，你看到了单任务 demo 的本质限制。

结论先说：

1. 你现在这个设计里，task 被包在 catch_unwind 中，确实只是“单执行流边界”，不是多线程框架。
2. unwind 天生是“当前正在 panic 的执行流”上的行为，不是“外部强行让另一个线程 unwind”。
3. 在 seL4 这类微内核里，要支持“按线程恢复”，需要每个线程自己有 unwind 边界（线程入口 trampoline），而不是一个全局 catch。

为什么不能随便对任意线程 unwind

1. DWARF/Itanium unwind 是同步异常模型。
2. 它依赖当前线程的栈帧、寄存器、LSDA 元数据逐帧回退。
3. 所以只能在该线程自己触发 panic 时展开。
4. 另一个线程无法安全地“远程帮它 unwind”；最多只能让它停机、重启，或发取消信号让它自己在安全点退出。

在 seL4 上的正确设计（线程级）

1. 每个用户线程入口统一走 thread_entry_trampoline。
2. trampoline 内部包一层 catch_unwind（或等价机制），这层就是该线程的 landing pad 归宿。
3. 线程内部任何函数 panic 后，panic_handler 启动 start_unwind_entry，最终只会落回该线程自己的 trampoline 恢复分支。
4. 恢复分支里做策略：
   - 清理线程资源
   - 上报 fault 到 supervisor
   - 选择重启线程或销毁线程
5. 其他线程不受影响，继续调度。

等价地说：不是“系统对某个线程做 unwind”，而是“该线程在自己的执行上下文里完成 unwind，系统只负责承接结果”。

协程怎么做

分两类：

1. 栈式协程（有独立栈）：
   - 和线程类似，可做每协程一个 unwind 边界。

2. 无栈协程（Rust async/Future，状态机）：
   - 没有独立调用栈可逐帧回退。
   - panic 发生在某次 poll 调用链中，只能 unwind 到 executor 在 poll 外层放的边界。
   - 所以通常做法是：executor 对每个任务 poll 包边界，panic 后将该任务标记失败并回收，不影响其他任务。

你这个 demo 如何升级为“线程化 Hopter 风格”

1. 把当前 task_run 抽象成 thread_run_trampoline。
2. main 不直接跑业务函数，而是调度多个线程入口（可先用伪线程/轮询模拟）。
3. 每个线程入口独立包边界并记录 thread_id。
4. panic_handler 中读取当前 thread_id，日志标出“哪条线程在展开”。
5. landing 后按 thread_id 回到对应恢复路径。

如果你愿意，我下一步可以直接把现有 demo 改成“多线程语义版框架”：
1. 支持多个 thread control block
2. 每线程独立 landing 路径
3. 演示线程 A panic 后 unwind，线程 B 继续执行
4. 输出接近 seL4 上 supervisor 处理 fault 的样式。



