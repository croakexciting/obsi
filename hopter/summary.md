# Hopter 学习总结

Hopter 学习记录总结，汇总所有相关的文章和进度

## 要点总结

- 主动栈检查功能只能通过修改 toolchain 的方式实现，使用过程宏无法实现，[主要原因](./hopter-toolchain-implementation.md#10-过程宏尝试及其根本局限)
- 软锁功能牺牲了锁的通用性，每个对象都要实现自己的下半部分，执行的下半部分是固定的，这样做带来了两个好处
  - hopter 中提到的，中断上下文中使用时，不需要关中断，消除中断延时 
  - 我测试发现，在高冲突场景下，由于 softlock 下半部分是批量执行的，所以具有一定性能优势。[测试说明](./softlock-std-demo/SOFTLOCK_BENCH_REPORT.md)

## 学习笔记

- [AArch64 no_std 内核 panic 恢复与栈保护系统设计（完整方案）](aarch64-panic-recovery-design.md)
- [hopter 栈检查，异常处理，任务恢复功能实现](hopter-toolchain-implementation.md)
- [hopter softlock 功能设计实现](hopter-softlock-analysis.md)
- [softlock 使用固定下半部分，相关设计实现](soft-lock-fixed-functions.md)
- [softlock x86 平台实现和效率测试](./softlock-std-demo/SOFTLOCK_BENCH_REPORT.md)