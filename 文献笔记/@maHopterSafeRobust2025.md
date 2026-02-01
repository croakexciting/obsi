# Hopter: a Safe, Robust, and Responsive Embedded Operating System

## Metadata
- **CiteKey**: {{citekey}}
 - **Type**: ConferencePaper
 - **Title**: Hopter: a Safe, Robust, and Responsive Embedded Operating System, 
 - **Author**: Ma, Zhiyao; Chen, Guojun; Chen, Zhuo; Zhong, Lin;  
- **Editor**: {{editor}};  
- **Translator**: {{translator}}
- **Publisher**: ACM,
- **Location**: Hilton Anaheim Anaheim CA USA,
- **Series**: {{series}}
- **Series Number**: {{seriesNumber}}
- **Journal**: {{publicationTitle}}, 
- **Volume**: {{volume}},
- **Issue**: {{issue}}
- **Pages**: 556-569
- **Year**: 2025 
- **DOI**: 10.1145/3711875.3729149
- **ISSN**: {{ISSN}}
- **ISBN**: 979-8-4007-1453-5

## Abstract
Microcontroller-based embedded systems are vulnerable to memory safety errors and must be robust and responsive because they are often used in unmanned and mission-critical scenarios. The Rust programming language offers an appealing compile-time solution for memory safety but leaves stack overflows unresolved and foils zero-latency interrupt handling. We present Hopter, a Rust-based embedded operating system (OS) that provides memory safety, system robustness, and interrupt responsiveness to embedded systems while requiring minimal application cooperation. Hopter executes Rust code under a novel finite-stack semantics that converts stack overflows into Rust panics, enabling recovery from fatal errors through stack unwinding and restart. Hopter also employs a novel mechanism called soft-locks so that the OS never disables interrupts. We compare Hopter with other well-known embedded OSes using controlled workloads and report our experience using Hopter to develop a flight control system for a miniature drone and a gateway system for Internet of Things (IoT). We demonstrate that Hopter is well-suited for resource-constrained microcontrollers and supports error recovery for real-time workloads.
## Files and Links
- **Url**: https://dl.acm.org/doi/10.1145/3711875.3729149
- **Uri**: http://zotero.org/users/19216654/items/M9GJKRDJ
- **Eprint**: {{eprint}}
- **File**: [PDF](file:////Users/croak/Zotero/storage/I9ZRLMUH/Ma%20%E7%AD%89%20-%202025%20-%20Hopter%20a%20Safe,%20Robust,%20and%20Responsive%20Embedded%20Operating%20System.pdf)
- **Local Library**: [Zotero](zotero://select/library/items/M9GJKRDJ)

## Tags and Collections
- **Keywords**: {{keywordsAll}}
- **Collections**: 嵌入式


----

## Comments


### 摘要

论文 Hopter: a Safe, Robust, and Responsive Embedded Operating System 发表在 MobiSys 2025 上，是一篇关于嵌入式操作系统设计与实现的研究成果。该工作由 Zhiyao Ma 等人提出，主要面向基于微控制器（MCU）的嵌入式系统，这类系统在无人机、物联网等任务关键场景中对 内存安全、鲁棒性（robustness）和实时响应性 有极高要求。传统 RTOS（实时操作系统）在这些方面存在不足，而 Hopter 提供了一个综合性的解决方案。



### 研究目标与动机

嵌入式系统往往资源受限，而且常用 C/C++ 开发，这会引入内存安全问题（如栈溢出）。此外，系统需要稳定可靠地运行，并保持极低的中断延迟来满足实时性需求。现有 RTOS 通常在下列方面存在挑战：

- 内存安全：C/C++ 缺乏语言级保护，容易出现内存错误；
- 栈溢出处理：传统 RTOS 很难安全检测和处理栈溢出；
- 实时中断响应：为了实现同步，系统往往需要在关键区禁用中断，这增加了中断延迟。

Hopter 的设计目标是同时解决上述问题，在 安全性、鲁棒性和响应性 三者之间找到平衡。同时对硬件资源没有要求，可以部署在各种类型的 mcu 芯片上


### 核心创新点
Hopter 的主要创新体现在三个关键技术层面：

#### 1. Rust 与有限栈语义（Finite‑Stack Semantics）
采用 Rust 语言开发，自然具备许多内存安全保障（所有权、借用检查等）。
引入 有限栈语义（finite‑stack semantics）：在执行函数前检查可用栈空间，若检测到栈空间不足，触发 Rust panic 而不是简单溢出，允许运行时通过 栈展开(stack unwinding) 和恢复机制避免系统崩溃。该功能类似代码插桩，在栈分配之前加入检查。
这种机制支持从潜在致命错误中恢复，提高系统鲁棒性。

📌 意义：传统 RTOS 很难自动检测栈溢出且通常导致系统崩溃，Hop­ter 则将栈溢出转为可恢复错误。同时有个好处是，可以立刻找到问题，而通常溢出导致的问题很难还原现场。
另一个重要意义时，这是纯软件实现，无需 mpu 这种硬件，可以部署在非常低端的 mcu 芯片上

#### 2. Soft‑Locks：无禁用中断的同步机制

为了实现高实时性和快速中断响应，Hopter 从不禁用中断（即内核不进入传统意义上的临界区）。
引入一种新的同步机制称为 soft‑locks，用于协调任务与中断之间的共享访问，而无需禁用中断。
这意味着在任何时候都有中断可以立即服务，避免了上下文切换和延迟增加。

soft-lock 核心思想是，中断可能会后获取锁，获取之后操作被记录，而不是卡在中断中，等到被抢占程序执行完前序操作后，再执行这些操作，保证不会出现死锁，所以自然无需禁用中断。

它有点类似于中断的上下部分设计，下半部分会推到后续执行，微内核上会在用户态执行。但是它的改进是，只在有需要的时候进行下半部分处理。

📌 意义：相比传统 RTOS 在关键区关闭中断这一传统设计，soft‑locks 机制在保持一致性与并发控制的同时，极大提升了中断响应性能。


#### 3. Panic 恢复与任务重启

利用 Rust 的 panic 机制，Hopter 在任务或中断处理器发生 panic 时能 清理资源并自动重启任务；
这种自动恢复机制增强了整个系统在面对错误时的 鲁棒性，避免单点 bug 导致系统整体失效。
核心是其实现的 unwinder 函数，根据当前栈进行调用展开，drop 所有栈上资源，确保再次执行时不会受影响。比如获取了一个锁，tock 上再次执行任务的时候就会卡死，而 hopter 不会

📌 意义：传统 RTOS 一旦任务崩溃，通常导致整个系统不可靠甚至崩溃；Hop­ter 则能通过重启策略保证系统持续运行。


### 性能与实验评估成果

“Table 2: Comparing Hopter to FreeRTOS and Tock, showing the overheads of Hopter’s components, and showing the overhead of implementing atomic update operations by disabling interrupts.”* [(Ma 等, 2025, p. 9)](zotero://open-pdf/library/items/I9ZRLMUH?page=9&annotation=)   


性能评估结果，可以看出 hopter 有以下特点

- 非常稳定和快速的中断响应，这点甚至比 freertos 还快
- 相比 tock，任务切换延时很低。当然没有 freertos 快，但是这是由于语言差别带来的
- hopter 不需要太多的 flash 和 sram 资源，不需要 mpu，可以部署在 M0 系列芯片上，同时即使有 mpu，其本身使用上也会增加消耗
- hopter 兼具相应快速，内存安全，健壮三个特点


- *“The drone rotates around 20 degrees along the yaw axis and drops a few centimeters when the Stabilizer task panics, but can still hover after the task restart. Stabilizer is the most sensitive to fatal errors because it directly modulates the power of the motors.”* [(Ma 等, 2025, p. 11)](zotero://open-pdf/library/items/I9ZRLMUH?page=11&annotation=)   

这个是让人印象深刻的实验，给飞控任务注入故障，可以很快检查到并恢复，不影响无人机


- *“Table 3: Binary size, SRAM usage, and CPU load of the flight control system. The SRAM usage excludes function call stacks, whose sizes are configurable. The CPU load is the average of 100 measurements when the drone is set to hover.”* [(Ma 等, 2025, p. 10)](zotero://open-pdf/library/items/I9ZRLMUH?page=10&annotation=)  
> *“表 3：飞控系统的二进制大小、SRAM 使用情况和 CPU 负载。 SRAM 的使用不包括函数调用堆栈，其大小是可配置的。 CPU 负载是无人机悬停时 100 次测量的平均值。**  

但是相比 freertos，由于使用 rust 已经栈的检查，CPU 使用率还是增加很多的，这是个问题。


### 总结：论文的贡献
Hopter 的主要贡献可以归纳如下：

- 创新性的内存安全设计：通过 Rust 和有限栈语义，将栈溢出转化为可恢复错误；
- 实时性与响应性保障：提出 soft‑locks 机制，在不禁用中断的前提下完成内核同步；
- 增强的鲁棒性：支持 panic 处理和任务自动重启；
- 实用性与低开销：在保持实时性和安全性的同时仍兼顾资源受限的嵌入式环境适用性。


### 个人的思考（感兴趣的点）

对于 rust 内存安全的增强修改是否有意义用在更复杂系统上，这种通过检查发现溢出的功能可以直接在现场发现问题，可以大大降低问题排查难度，即使不考虑恢复之类，也很有价值，不知道目前有没有类似的。目前的 unwind 方式还是需要触发 panic 之后，而此时可能溢出现场已经破坏了。同样的，这种方式能否用于其他内存问题场景，rust 对于这些已经支持很完善，那么能否用在 c 上，但是这涉及到编译器知识了。还有，hopter 中的任务/模块 是以某个函数作为主体，通用操作系统中会更复杂一些

soft-lock 能否移到更复杂的 OS 中，hopter 中只是用其解决中断和任务之间的同步，任务和任务之前根据其设计是不会发生锁竞争的。用法比较简单，如果移到更复杂 OS 中会有挑战吧，而且实时性系统是否需要完全 0 延迟中断也是个问题，我理解优化到一定程度后，就已经满足绝大部分场景需求了

目前基于 rust 的各种方法，把操作系统服务做的越来越安全，微内核这种完全隔离的架构是否还有意义，可以考虑将可信，或者经过测试认证的模块放到一起。只是说客户或者开发的驱动服务之类的还是在用户态，弱化内核和服务之间的隔离界限


