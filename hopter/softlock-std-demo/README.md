# softlock-std-demo

A runnable x86 Rust demo using Tokio coroutines (`tokio::spawn`) that ports the core Hopter Soft-Lock idea:

- `Full` path: one winner gets heavy access.
- `PendOnly` path: losers only record intent.
- `Drop` drain loop: winner consumes all pended intents before fully releasing.

## Run (Coroutine / Async)

```bash
cargo run
```

## What This Demonstrates

Under high async concurrency, many operations go through `PendOnly` and are then consumed in batches by `run_pended_op`.
This often reduces expensive lock-holding work frequency (coalescing effect).

Printed metrics include:

- full path entries
- pend-only entries
- drained pending items
- drain batches
- average items per drain batch

If `avg items per drain` is greater than `1.0`, batching is happening.

## Notes About Async Tasks vs Hopter ISR Model

Hopter's original proof relies on ISR semantics: preemptor finishes before returning.
In desktop async runtimes, tasks run on worker threads and can race arbitrarily. This demo stays correct because shared state uses atomics + mutexes and the drain protocol, but behavior differs:

- Can improve throughput by batching pending operations.
- Can hurt tail latency/fairness under heavy contention due to retries/spin.
- Is not a drop-in replacement for all lock-based critical sections.

So the extra effect you suspected is real: it acts like a lightweight event coalescer under contention.



我会基于现有 softlock-std-demo 做一组可复现实验，对比“直接锁路径”与“defer+批量回放路径”在不同冲突模式下的吞吐和均值耗时，只评价锁侧性能，不计中断延时语义。

Read [](file:///home/cwyz/workspace/hopter/softlock-std-demo)

Read [](file:///home/cwyz/workspace/hopter/softlock-std-demo/Cargo.toml)

Read [](file:///home/cwyz/workspace/hopter/softlock-std-demo/src/main.rs)

我会把 main.rs 改成可跑多场景基准：同一工作负载下对比 `SoftLock(defer+batch)` 与 `Mutex(直接加锁)`，然后实际运行给你结论。

Generating patch (409 lines) in [](file:///home/cwyz/workspace/hopter/softlock-std-demo/src/main.rs)


```

```
代码已改成多场景基准了；下一步我会直接跑 release benchmark，给你一组“什么场景变好/变差”的实测结果和原因解释。

Ran terminal command:  cd /home/cwyz/workspace/hopter/softlock-std-demo && cargo run --release

Checked terminal output

前两组结果已经出来了：在当前 x86 线程模型里 SoftLock 吞吐分别是 Mutex 的 0.97x 和 0.81x。 我再取第三组（重临界区）结果，重点看批量回放是否出现优势。

Checked terminal output

Checked terminal output

Killed terminal

第三组参数导致运行过慢，我会做一个小修正：只在消费者这轮确实处理了工作时才施加 `hold_spin`，然后重跑，能更准确反映“重临界区下批量回放是否收益”。

Generating patch (58 lines) in [](file:///home/cwyz/workspace/hopter/softlock-std-demo/src/main.rs)


```

```


Ran terminal command:  cd /home/cwyz/workspace/hopter/softlock-std-demo && cargo run --release

已完成验证，并把基准程序改成多场景对比版，位置在 main.rs。

我实际跑了 release 基准（同一工作负载下对比 SoftLock defer+batch 与 Mutex 直接加锁），结果如下：

1. 低冲突，临界区很短  
SoftLock: 7.425 Mops/s  
Mutex: 8.328 Mops/s  
比值: 0.89x（SoftLock 更慢）

2. 高冲突，临界区很短  
SoftLock: 6.320 Mops/s  
Mutex: 7.993 Mops/s  
比值: 0.79x（SoftLock 更慢）

3. 高冲突，Full 持有者临界区很重  
SoftLock: 1.669 Mops/s  
Mutex: 0.037 Mops/s  
比值: 45.12x（SoftLock 大幅更快）

结论（只看锁吞吐，不看中断延时语义）：

1. 你的直觉是对的，批量处理在某些场景会明显变好。  
当 Full 持有者临界区很重、生产者并发很多时，PendOnly 记账 + 批量回放可以把大量细粒度争用压缩成较少批次，吞吐显著提升。

2. 但在轻临界区场景，SoftLock 往往不占优。  
主要被额外的原子操作、分支和 pending 清算循环成本拖累；Mutex 直接路径更短。

3. 所以它不是“普遍更快”，而是“冲突形态相关”。  
轻冲突/轻临界区：通常变慢。  
重冲突/重临界区且可批量回放：可能显著变快。

补充说明：
这套实验是 x86 多线程上的吞吐对比，刻意忽略了中断响应语义，与你这次问题目标一致。  
如果你愿意，我下一步可以再加一个参数扫描模式（自动扫 producer 数、consumer 持锁长度、batch 大小），输出一张“何时从亏转盈”的拐点表。

Made changes.