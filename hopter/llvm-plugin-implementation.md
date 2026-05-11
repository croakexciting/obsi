# 用 LLVM 插件实现 Hopter 机制

> 本文档记录尝试将 Hopter 中原本依赖 LLVM/rustc 补丁实现的机制，改用**树外 LLVM IR Pass 插件**的方式重新实现的设计思路与进展。
>
> 核心动机：Hopter 目前需要维护一套 fork 的编译器工具链，每次 LLVM/Rust 版本升级都要重新移植补丁。如果能用插件实现等价的功能，就可以使用标准发行版工具链，极大降低维护成本。

---

## 目录

- [用 LLVM 插件实现 Hopter 机制](#用-llvm-插件实现-hopter-机制)
  - [目录](#目录)
  - [1. 背景：Hopter 依赖哪些编译器补丁](#1-背景hopter-依赖哪些编译器补丁)
  - [2. 插件方式的核心约束](#2-插件方式的核心约束)
  - [3. Demo 1：栈边界检查](#3-demo-1栈边界检查)
    - [与 Hopter 树内方案的差距](#与-hopter-树内方案的差距)

---

## 1. 背景：Hopter 依赖哪些编译器补丁

Hopter 在 Cortex-M 上的实现依赖以下三类编译器改动（详见 `hopter-toolchain-implementation.md`）：

| 机制 | 当前方式 | 插件可行性 |
|---|---|---|
| **分段栈检查**——每个函数入口插入 `svc #255` | rustc/LLVM 树内补丁，精确获取帧大小 | ✅ IR Pass 可插入检查，帧大小略过估算（可接受） |
| **Drop handler 检查**——每个 drop handler 额外插入 `svc #254` | rustc 补丁识别 drop glue | ⚠️ IR Pass 无法区分 drop glue 与普通函数，需借助函数名 heuristic |
| **Unwind 协调**——在 `_Unwind_Resume` 和 landing pad 前后插入 `svc #252/#253` | rustc EH personality 改动 | ❌ IR Pass 无法拦截 EH code generation，此项目无法绕过 |

本文档聚焦于**可行的部分**：以连续栈 + 软边界的方式实现栈检查，作为第一个 demo。

---

## 2. 插件方式的核心约束

**IR Pass 在后端之前运行**，这带来两个影响：

1. **不知道精确帧大小**：callee-save 寄存器数量、spill、stack canary、对齐填充均在寄存器分配和帧降低之后才确定，IR 时不可见。
2. **序言先于检查执行**：后端把 callee-save `stp` 和静态 `alloca sub sp` 生成为函数的**第一批机器指令**，比 IR Pass 插入的检查代码更早运行。

**解决方案（本插件的核心洞见）**：

- 问题 1 不需要解决——当 IR 级检查执行时，序言已经跑完，直接读 SP 就能得到包含所有开销的真实值。
- 问题 2 通过"调用者的检查为被调函数的序言托底"解决：调用者的检查保证其栈底距软边界至少还有 `kFramePadding` 字节，被调函数的序言写入不会越界。

---

## 3. Demo 1：栈边界检查

> 实现代码：`aarch64-split-stack-plugin/`
> 详细设计见 [[aarch64-split-stack-plugin/README.zh.md]]

### 与 Hopter 树内方案的差距

| 项目 | 树内补丁 | 本 Demo |
|---|---|---|
| 帧大小精度 | 精确（`MachineFrameInfo`） | 序言后 SP 直读，实际上也精确 |
| 溢出处理 | 分配新 stacklet，entry-restart | abort（连续栈，无分段） |
| Drop handler 区分 | 独立 `svc #254` | 未实现 |
| Unwind 协调 | `svc #252/#253` | 未实现（IR Pass 无法做到） |
| 工具链依赖 | fork LLVM + rustc | 标准 clang-18，无需修改 |
| 目标平台 | Cortex-M（thumb2） | aarch64-linux（QEMU 验证） |
