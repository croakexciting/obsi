# SoftLock 基准实验报告（std/x86）

本文档总结三件事：

1. 本项目里 SoftLock 的实现细节（不是概念描述，而是代码级设计）
2. 实验如何设计、如何保证对比公平、每个参数代表什么
3. 实测结果与结论：哪些场景变好，哪些场景变差

范围声明：本文只讨论“锁侧吞吐性能”，刻意不纳入“关中断/中断延时”语义收益。

---

## 1. 实现设计（详细）

代码位置：`src/main.rs`

### 1.1 协议抽象

该 demo 复用了 Hopter 的核心抽象：

- `AllowPendOp<'a>`：定义两种访问器
  - `FullAccessor`
  - `PendOnlyAccessor`
- `RunPendedOp`：定义 Full 路径如何回放 deferred 工作
- `Access` 枚举：一次访问只能落到
  - `Access::Full`
  - `Access::PendOnly`

这三个抽象把“访问权限”和“回放职责”在类型层面明确分离。

### 1.2 SoftLock 结构与状态机

`SoftLock<T>` 的最小状态：

- `locked: AtomicBool`
  - 表示当前是否已有 Full 持有者
- `pending: AtomicBool`
  - 表示是否存在待回放工作
- `content: T`
  - 业务对象（本 demo 中是 `DemoInner`）

访问流程：

1. `AccessGuard::guard` 尝试 `CAS(locked: false -> true)`
2. CAS 成功：本次访问获得 Full
3. CAS 失败：本次访问获得 PendOnly

这意味着“高并发下不会都去抢重路径”，失败者可降级为记录意图。

### 1.3 Guard drop 阶段的回放循环

这是实现里最关键的部分。若本次是 Full 持有者，`drop` 会执行：

1. `prev_pending = pending.swap(false)`
2. 若 `prev_pending == true`，执行 `run_pended_op()`
3. 释放 `locked = false`
4. 再检查 `pending`
5. 若期间有新 pending，则重新 CAS 抢回 Full 并继续循环

核心目的：避免“回放窗口中又出现新 defer”导致丢失。

若本次是 PendOnly 访问者，`drop` 只做一件事：`pending = true`。

### 1.4 业务对象 DemoInner 如何映射 defer

`DemoInner` 同时包含“轻路径记录”和“重路径执行”两类状态：

- defer 记录侧
  - `pending_submit: AtomicUsize`
- 重路径执行侧
  - `queue: Mutex<VecDeque<u64>>`
  - `next_id: AtomicU64`

含义：

- Full 路径可以直接把任务（这里用 id 表示）入队
- PendOnly 路径不碰队列，只做 `pending_submit += 1`
- `run_pended_op` 一次性 `swap(0)` 拿走待处理计数并批量入队

这正是“先记账，再批量回放”的实现化。

### 1.5 Full 与 PendOnly 的职责边界

Full 路径（`InnerFullAccessor`）负责：

- `submit_direct()`：直接入队
- `process_some(budget)`：消费队列项
- `run_pended_op()`：批量回放 defer

PendOnly 路径（`InnerPendAccessor`）负责：

- `pend_submit()`：仅递增 `pending_submit`

这个边界是实验成立的基础：
- 轻路径足够轻
- 重路径可批量化

### 1.6 对照组（Mutex）设计

为了公平比较，基线组 `MutexInner` 使用同等业务语义：

- 生产者每次提交都直接 `lock(queue)` 后入队
- 消费者也 `lock(queue)` 后按 budget 消费

区别仅在于：

- SoftLock：争用失败者走 defer 记录
- Mutex：争用失败者继续等待锁

因此对比聚焦在“冲突处理策略”而非业务逻辑差异。

---

## 2. 实验设计

### 2.1 实验目标

验证问题：

- 在不考虑中断延时语义时，SoftLock 的 defer+batch 机制何时更快/更慢？

### 2.2 指标定义

统一指标：

- `elapsed`：总耗时
- `throughput (Mops/s)`：
  - $\text{throughput} = \frac{\text{total ops}}{\text{elapsed seconds} \times 10^6}$

SoftLock 额外观测：

- `full_entries`：落到 Full 的次数
- `pend_entries`：落到 PendOnly 的次数
- `drained_items`：回放总条目数
- `drained_batches`：回放批次数
- `avg_batch = drained_items / drained_batches`

Mutex 额外观测：

- `producer_lock_acq`
- `consumer_lock_acq`

这些指标可以直接解释“批量化是否发生、发生到什么程度”。

### 2.3 场景参数

场景通过 `Scenario` 结构体配置：

- `producers`
- `ops_per_producer`
- `producer_pause_every`
- `consumer_budget`
- `consumer_hold_spin`

其中 `consumer_hold_spin` 用于模拟 Full 持有者临界区重负载（额外持有成本）。

### 2.4 三组实验场景

1. 低冲突 + 轻临界区
2. 高冲突 + 轻临界区
3. 高冲突 + 重临界区（通过 `hold_spin=700`）

### 2.5 正确性校验

每组都做一致性断言：

- `processed == total_ops`
- `pending == 0`（SoftLock）
- `queue_left == 0`

防止“快是因为丢工作”的伪结论。

### 2.6 运行方法

```bash
cd softlock-std-demo
cargo run --release
```

---

## 3. 实测结果

以下结果来自一次完整 release 运行。

### 场景 A：低冲突，轻临界区

- 参数：`producers=2, ops/producer=200000, hold_spin=0`
- SoftLock：`7.425 Mops/s`
- Mutex：`8.328 Mops/s`
- 比值（SoftLock/Mutex）：`0.89x`

结论：SoftLock 略慢。

### 场景 B：高冲突，轻临界区

- 参数：`producers=8, ops/producer=200000, hold_spin=0`
- SoftLock：`6.320 Mops/s`
- Mutex：`7.993 Mops/s`
- 比值（SoftLock/Mutex）：`0.79x`

结论：SoftLock 明显慢于 Mutex。

### 场景 C：高冲突，重临界区

- 参数：`producers=8, ops/producer=200000, hold_spin=700`
- SoftLock：`1.669 Mops/s`
- Mutex：`0.037 Mops/s`
- 比值（SoftLock/Mutex）：`45.12x`

结论：SoftLock 大幅领先。

---

## 4. 结果解读

### 4.1 为什么前两组 SoftLock 会慢

在轻临界区下，SoftLock 的额外机制成本更显著：

1. CAS + 分流判断
2. pending 标志维护
3. drop 期回放检查

此时 Mutex 的直接路径更短，吞吐更高。

### 4.2 为什么第三组 SoftLock 会快很多

当 Full 持有者很重、并发争用高时：

1. 大量请求走 PendOnly（轻操作）
2. 回放阶段按批处理（摊销重路径成本）
3. 避免了每次提交都在重临界区内串行锁竞争

因此批量化优势被放大。

---

## 5. 结论（只针对锁吞吐）

1. SoftLock 不是普遍更快
2. 轻临界区场景通常不占优
3. 在“高争用 + 可批量回放 + Full 路径重”的形态下，可显著优于直接锁

也就是说，性能好坏由工作负载形状决定，不是单向结论。

---

## 6. 边界与注意事项

1. 本实验平台是 std/x86 线程模型，不是 Cortex-M ISR 语义模型
2. 本文结论只回答“锁吞吐”问题，不回答“中断实时性”问题
3. 数值会随机器变化，但趋势具有参考意义

---

## 7. 下一步可扩展实验（建议）

为了更系统地得到“拐点”，可继续做参数扫描：

1. 扫描 `producers`（并发度）
2. 扫描 `consumer_hold_spin`（Full 持有成本）
3. 扫描 `consumer_budget`（批大小上限）

输出二维热图或表格后，就能得到“SoftLock 何时从亏转盈”的边界线。

---

## 8. 逐函数对照表（报告概念 <-> 源码位置）

下面这张表用于快速把本文术语映射回 `src/main.rs` 的函数实现。

| 报告中的概念 | 源码函数/类型 | 作用说明 |
|---|---|---|
| 双访问权限模型 | `trait AllowPendOp<'a>`、`enum Access` | 定义 Full 与 PendOnly 两种能力边界 |
| deferred 回放协议 | `trait RunPendedOp` | 定义 Full 持有者如何补做挂起工作 |
| 访问分流入口 | `SoftLock::with_access` | 一次访问在运行时被分流为 Full 或 PendOnly |
| CAS 抢占 Full | `AccessGuard::guard` | 用 `locked.compare_exchange` 决定是否获得 Full |
| Full 释放时清算 | `impl Drop for AccessGuard` | `pending.swap(false)` + `run_pended_op` + 重新抢锁循环 |
| PendOnly 侧登记完成 | `impl Drop for AccessGuard`（`lock_held == false` 分支） | 设置 `pending=true`，通知 Full 侧后续补做 |
| 业务对象（SoftLock 版本） | `struct DemoInner` | 持有 `pending_submit` 与 `queue` 等状态 |
| Full 直接提交 | `InnerFullAccessor::submit_direct` | 直接入队，模拟“立即执行”路径 |
| Full 消费任务 | `InnerFullAccessor::process_some` | 消费队列元素，模拟消费者重路径 |
| Full 批量回放 | `impl RunPendedOp for InnerFullAccessor::run_pended_op` | `swap(0)` 取出 defer 计数并批量入队 |
| PendOnly 仅记账 | `InnerPendAccessor::pend_submit` | 仅原子递增 `pending_submit` |
| 基线对象（Mutex 版本） | `struct MutexInner` | 同等业务语义的直接锁实现 |
| SoftLock 基准执行器 | `run_softlock_bench` | 启动 producer/consumer 并采集 SoftLock 指标 |
| Mutex 基准执行器 | `run_mutex_bench` | 启动 producer/consumer 并采集 Mutex 指标 |
| 场景定义 | `struct Scenario` | 控制并发度、批量预算、重负载强度 |
| 单场景对比输出 | `run_scenario` | 输出两种实现吞吐与关键统计 |
| 批量实验入口 | `main` | 构建 3 个场景并依次执行 |

### 8.1 读代码建议顺序

建议按下面顺序阅读 `src/main.rs`，理解成本最低：

1. 看抽象层：`AllowPendOp`、`RunPendedOp`、`Access`
2. 看协议层：`SoftLock`、`AccessGuard::guard`、`Drop for AccessGuard`
3. 看业务层：`DemoInner`、`InnerFullAccessor`、`InnerPendAccessor`
4. 看回放实现：`run_pended_op`
5. 看实验器：`run_softlock_bench` 与 `run_mutex_bench`
6. 看场景入口：`run_scenario` 与 `main`

按这个顺序可以先建立机制，再看业务映射，最后看实验框架与数据输出。
