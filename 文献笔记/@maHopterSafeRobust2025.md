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



----

## Extracted Annotations


论文 Hopter: a Safe, Robust, and Responsive Embedded Operating System 发表在 MobiSys 2025 上，是一篇关于嵌入式操作系统设计与实现的研究成果。该工作由 Zhiyao Ma 等人提出，主要面向基于微控制器（MCU）的嵌入式系统，这类系统在无人机、物联网等任务关键场景中对 内存安全、鲁棒性（robustness）和实时响应性 有极高要求。传统 RTOS（实时操作系统）在这些方面存在不足，而 Hopter 提供了一个综合性的解决方案。



🎯 研究目标与动机
嵌入式系统往往资源受限，而且常用 C/C++ 开发，这会引入内存安全问题（如栈溢出）。此外，系统需要稳定可靠地运行，并保持极低的中断延迟来满足实时性需求。现有 RTOS 通常在下列方面存在挑战：




内存安全：C/C++ 缺乏语言级保护，容易出现内存错误；


栈溢出处理：传统 RTOS 很难安全检测和处理栈溢出；


实时中断响应：为了实现同步，系统往往需要在关键区禁用中断，这增加了中断延迟。


Hopter 的设计目标是同时解决上述问题，在 安全性、鲁棒性和响应性 三者之间找到平衡。同时对硬件资源没有要求，可以部署在各种类型的 mcu 芯片上



💡 核心创新点
Hopter 的主要创新体现在三个关键技术层面：


🧠 1. Rust 与有限栈语义（Finite‑Stack Semantics）


采用 Rust 语言开发，自然具备许多内存安全保障（所有权、借用检查等）。


引入 有限栈语义（finite‑stack semantics）：在执行函数前检查可用栈空间，若检测到栈空间不足，触发 Rust panic 而不是简单溢出，允许运行时通过 栈展开(stack unwinding) 和恢复机制避免系统崩溃。该功能类似代码插桩，在栈分配之前加入检查。


这种机制支持从潜在致命错误中恢复，提高系统鲁棒性。


📌 意义：传统 RTOS 很难自动检测栈溢出且通常导致系统崩溃，Hop­ter 则将栈溢出转为可恢复错误。同时有个好处是，可以立刻找到问题，而通常溢出导致的问题很难还原现场。


另一个重要意义时，这是纯软件实现，无需 mpu 这种硬件，可以部署在非常低端的 mcu 芯片上



⚡ 2. Soft‑Locks：无禁用中断的同步机制


为了实现高实时性和快速中断响应，Hopter 从不禁用中断（即内核不进入传统意义上的临界区）。


引入一种新的同步机制称为 soft‑locks，用于协调任务与中断之间的共享访问，而无需禁用中断。


这意味着在任何时候都有中断可以立即服务，避免了上下文切换和延迟增加。


soft-lock 核心思想是，中断可能会后获取锁，获取之后操作被记录，而不是卡在中断中，等到被抢占程序执行完前序操作后，再执行这些操作，保证不会出现死锁，所以自然无需禁用中断。


📌 意义：相比传统 RTOS 在关键区关闭中断这一传统设计，soft‑locks 机制在保持一致性与并发控制的同时，极大提升了中断响应性能。



🛠️ 3. Panic 恢复与任务重启


利用 Rust 的 panic 机制，Hopter 在任务或中断处理器发生 panic 时能 清理资源并自动重启任务；


这种自动恢复机制增强了整个系统在面对错误时的 鲁棒性，避免单点 bug 导致系统整体失效。


核心是其实现的 unwinder 函数，根据当前栈进行调用展开，drop 所有栈上资源，确保再次执行时不会受影响。比如获取了一个锁，tock 上再次执行任务的时候就会卡死，而 hopter 不会


📌 意义：传统 RTOS 一旦任务崩溃，通常导致整个系统不可靠甚至崩溃；Hop­ter 则能通过重启策略保证系统持续运行。

- *“📊 性能与实验评估成果
“Table 2: Comparing Hopter to FreeRTOS and Tock, showing the overheads of Hopter’s components, and showing the overhead of implementing atomic update operations by disabling interrupts.”* [(Ma 等, 2025, p. 9)](zotero://open-pdf/library/items/I9ZRLMUH?page=9&annotation=)   


性能评估结果，可以看出 hopter 有以下特点




非常稳定和快速的中断响应，这点甚至比 freertos 还快


相比 tock，任务切换延时很低。当然没有 freertos 快，但是这是由于语言差别带来的


hopter 不需要太多的 flash 和 sram 资源，不需要 mpu，可以部署在 M0 系列芯片上，同时即使有 mpu，其本身使用上也会增加消耗


hopter 兼具相应快速，内存安全，健壮三个特点




- *“The drone rotates around 20 degrees along the yaw axis and drops a few centimeters when the Stabilizer task panics, but can still hover after the task restart. Stabilizer is the most sensitive to fatal errors because it directly modulates the power of the motors.”* [(Ma 等, 2025, p. 11)](zotero://open-pdf/library/items/I9ZRLMUH?page=11&annotation=)   


这个是让人印象深刻的实验，给飞控任务注入故障，可以很快检查到并恢复，不影响无人机



🧠 总结：论文的贡献
Hopter 的主要贡献可以归纳如下：




创新性的内存安全设计：通过 Rust 和有限栈语义，将栈溢出转化为可恢复错误；


实时性与响应性保障：提出 soft‑locks 机制，在不禁用中断的前提下完成内核同步；


增强的鲁棒性：支持 panic 处理和任务自动重启；


实用性与低开销：在保持实时性和安全性的同时仍兼顾资源受限的嵌入式环境适用性。


个人的思考（感兴趣的点）


对于 rust 内存安全的增强修改是否有意义用在更复杂系统上，这种通过检查发现溢出的功能可以直接在现场发现问题，可以大大降低问题排查难度，即使不考虑恢复之类，也很有价值，不知道目前有没有类似的。目前的 unwind 方式还是需要触发 panic 之后，而此时可能溢出现场已经破坏了。同样的，这种方式能否用于其他内存问题场景，rust 对于这些已经支持很完善，那么能否用在 c 上，但是这涉及到编译器知识了


soft-lock 能否移到更复杂的 OS 中，hopter 中只是用其解决中断和任务之间的同步，任务和任务之前根据其设计是不会发生锁竞争的。用法比较简单，如果移到更复杂 OS 中会有挑战吧，而且实时性系统是否需要完全 0 延迟中断也是个问题，我理解优化到一定程度后，就已经满足绝大部分场景需求了


目前基于 rust 的各种方法，把操作系统服务做的越来越安全，微内核这种完全隔离的架构是否还有意义，可以考虑将可信，或者经过测试认证的模块放到一起。只是说客户或者开发的驱动服务之类的还是在用户态，弱化内核和服务之间的隔离界限


用宏的形式实现插桩


报告 ppt


先在 rust 中重现，通用场景重现。然后看看用在 qnx


软锁机制，通用性更高，需要思考下在哪使用。rcu 机制了解


总体步骤




在更通用的 rust 程序中使用栈检查机制，复现栈检查和恢复的工作


了解目前对于栈溢出/内存安全检查，排查方法和机制


在 QNX 上开发 rust 应用，使用这种机制


思考软锁在微内核中的应用


额外的思考，操作系统服务如何放到 supervisor 中调用，也许作为 vm 调用？


基础是 QNX + Rust 使用，目标是提高可靠性，易用性，安全性


可以想到的一个场景是，比如 seL4 上的应用分配了固定大小的内存，导致每个线程栈不能分配过大，可能出现溢出





不同进程之间共享锁的释放可能是一个点







- *“The Rust programming language offers an appealing compile-time solution for memory safety but leaves stack overflows unresolved and foils zero-latency interrupt handling.”* [(Ma 等, 2025, p. 1)](zotero://open-pdf/library/items/I9ZRLMUH?page=1&annotation=)  
> *“Rust 编程语言为内存安全提供了一个有吸引力的编译时解决方案，但未解决堆栈溢出问题并阻碍零延迟中断处理。**  


rust 很早就宣称用于嵌入式领域，虽然 rust 是内存安全，但是 unsafe，栈溢出是无法通过编译器检查出来的




- *“Hopter executes Rust code under a novel finite-stack semantics that converts stack overflows into Rust panics, enabling recovery from fatal errors through stack unwinding and restart.”* [(Ma 等, 2025, p. 1)](zotero://open-pdf/library/items/I9ZRLMUH?page=1&annotation=)  
> *“Hopter 在一种新颖的有限堆栈语义下执行 Rust 代码，该语义将堆栈溢出转换为 Rust 恐慌，从而能够通过堆栈展开和重新启动从致命错误中恢复。**  


可以解决 stackoverflow 的问题




- *“Hopter also employs a novel mechanism called soft-locks so that the OS never disables interrupts.”* [(Ma 等, 2025, p. 1)](zotero://open-pdf/library/items/I9ZRLMUH?page=1&annotation=)  
> *“Hopter 还采用了一种称为软锁的新颖机制，以便操作系统永远不会禁用中断。**  


这个有点意思，是在任何情况都不禁用中断吗，但是在复杂系统上能用吗




- *“We demonstrate that Hopter is well-suited for resource-constrained microcontrollers and supports error recovery for real-time workloads.”* [(Ma 等, 2025, p. 1)](zotero://open-pdf/library/items/I9ZRLMUH?page=1&annotation=)  
> *“我们证明 Hopter 非常适合资源受限的微控制器，并支持实时工作负载的错误恢复。**  


第一印象就是，在 mcu rtos 这种看起来非常成熟简单的领域也可以做研究发文章




- *“it is imperative to enhance their memory safety and system robustness without demanding more resources or sacrificing system responsiveness.”* [(Ma 等, 2025, p. 1)](zotero://open-pdf/library/items/I9ZRLMUH?page=1&annotation=)   


嵌入式 rtos 是单进程的，就是一个操作系统一直在运行，所以只要某个功能 panic，那整个系统就直接挂掉




- *“The Rust programming language offers an attractive alternative to C. It guarantees memory safety primarily through compile-time checks, incurring little runtime overhead, and has already seen adoption in operating system (OS) development [8, 13, 14, 26, 50, 56], including embedded systems [41, 55, 63].”* [(Ma 等, 2025, p. 1)](zotero://open-pdf/library/items/I9ZRLMUH?page=1&annotation=)  
> *“Rust 编程语言提供了一种有吸引力的 C 替代方案。它主要通过编译时检查来保证内存安全，几乎不会产生运行时开销，并且已经在操作系统 (OS) 开发中得到采用 [8,13,14,26,50,56]，包括嵌入式系统 [41,55,63]。**  


几年前就看到 rust 用到 rtos 中的例子了




- *“First, memory safety errors linger on even with safe Rust, because its semantics assumes an infinite size of function call stacks,”* [(Ma 等, 2025, p. 1)](zotero://open-pdf/library/items/I9ZRLMUH?page=1&annotation=)  
> *“首先，即使使用安全的 Rust，内存安全错误仍然存​​在，因为它的语义假设函数调用堆栈的大小是无限的，**  


这是无法检查栈溢出的原因，还比如你在一个循环了一直申请内存，rust 是检查不出来的




- *“which is especially problematic for microcontroller-based embedded systems where there is no virtual memory and only 10s to 100s KiB of SRAM are usually available.”* [(Ma 等, 2025, p. 1)](zotero://open-pdf/library/items/I9ZRLMUH?page=1&annotation=)  
> *“对于基于微控制器的嵌入式系统来说，这尤其成问题，因为这些系统没有虚拟内存，通常只有 10 到 100 KiB 的 SRAM 可用。**  


如果在应用中，虚拟内存很大，如果物理内存不够，操作系统也会报错，mapping 失败




- *“Second, Rust’s built-in exception handling mechanism, i.e., panics, requires a stack unwinder, which is usually unavailable on microcontrollers. Without a stack unwinder, panics lead to hang or reset of the application [41] or the whole system [11, 12].”* [(Ma 等, 2025, p. 1)](zotero://open-pdf/library/items/I9ZRLMUH?page=1&annotation=)  
> *“其次，Rust 的内置异常处理机制（即恐慌）需要堆栈展开器，而这在微控制器上通常不可用。如果没有堆栈展开器，恐慌会导致应用程序 [41] 或整个系统 [11, 12] 挂起或重置。**  


这个应该是 kernel 都存在的问题吧，存在疑问？




- *“Known solutions [47, 48, 57, 58] are infeasible with safe Rust because they struggle to pass the compile-time check. §2 elaborates on the challenges.”* [(Ma 等, 2025, p. 1)](zotero://open-pdf/library/items/I9ZRLMUH?page=1&annotation=)  
> *“已知的解决方案 [47,48,57,58] 对于安全 Rust 来说是不可行的，因为它们很难通过编译时检查。 §2 详细阐述了挑战。**  


不关中断的话，rust 检查无法通过？零延时中断和 rust 编译检查有什么关系，接着看吧。




- *“Hopter augments Rust with finite-stack semantics (FS-semantics) to achieve stack memory safety and overflow resilience”* [(Ma 等, 2025, p. 1)](zotero://open-pdf/library/items/I9ZRLMUH?page=1&annotation=)  
> *“Hopter 通过有限堆栈语义（FS-semantics）增强 Rust，以实现堆栈内存安全和溢出弹性**  


通过修改 rust 实现堆栈安全检查，个人觉得通用性还比较高，是别人也可以用的工作




- *“Hopter will temporarily extend the stack to finish the drop handler [35, 45, 67] and raise a panic afterward. This effectively unifies stack overflows with other fatal errors using Rust panics.”* [(Ma 等, 2025, p. 2)](zotero://open-pdf/library/items/I9ZRLMUH?page=2&annotation=)  
> *“Hopter 将暂时扩展堆栈以完成掉落处理程序 [35, 45, 67] 并随后引发恐慌。这可以有效地将堆栈溢出与其他使用 Rust 恐慌的致命错误统一起来。**  


这块看的不是很懂，不太懂 rust 本身错误处理和语言特性




- *“Hopter then reclaims the resources upon panics utilizing a customized stack unwinder, which allows Hopter to automatically restart failed tasks”* [(Ma 等, 2025, p. 2)](zotero://open-pdf/library/items/I9ZRLMUH?page=2&annotation=)  
> *“然后，Hopter 在发生恐慌时利用定制的堆栈展开器回收资源，这允许 Hopter 自动重新启动失败的任务**  


可以处理软件 panic，然后保证系统不挂，重启任务，已经很有价值了




- *“When possible, Hopter performs a concurrent restart to expedite recovery, where the restarted task instance runs concurrently with the unwinding procedure of the failed one.”* [(Ma 等, 2025, p. 2)](zotero://open-pdf/library/items/I9ZRLMUH?page=2&annotation=)  
> *“如果可能，Hopter 会执行并发重新启动以加速恢复，其中重新启动的任务实例与失败任务实例的展开过程同时运行。**  


unwinding 是在某个线程中执行吗？这个过程是什么，需要详细了解下


1️⃣ unwind 本质上是什么？
✅ unwind = 一段程序在“反向走调用栈”
它是：




正在运行的代码


有 PC / SP / 寄存器变化


会访问内存


会有分支、循环


不是：




硬件自动行为


魔法回溯


“CPU 帮你想起来的”


你可以把它理解成：



“一段解释‘如何退回上一帧’的程序”






通过几方面的创新，最终实现了一个系统解决了一类实际问题，达到了很好的效果




- *“Hopter requires a customized compiler to compile the system, but the Rust syntax remains the same and the semantics compatible. Hopter also supports unmodified third-party hardware abstraction layer (HAL) crates, allowing application programmers to tap the growing Rust ecosystem.”* [(Ma 等, 2025, p. 2)](zotero://open-pdf/library/items/I9ZRLMUH?page=2&annotation=)   


很大的价值，看起来完全是可以在现实中用起来，而不只是用于研究




- *“• Finite-stack semantics for Rust to guarantee stack memory safety and overflow resilience, implemented via compile-time instrumentation and OS support. • Soft-locks, a novel synchronization primitive which enables zerolatency interrupt handling on Rust-based systems with threaded tasks. • Open-source implementation of the Hopter embedded OS [25], which integrates FS-semantics and soft-locks to deliver memory safety, failure resilience, and responsiveness, while requiring minimal application cooperation. • Evaluation of the code size and performance overhead incurred by FS-semantics, the unwinding mechanism, and soft-locks, showing Hopter’s suitability for resource-constrained microcontrollers and real-time workloads”* [(Ma 等, 2025, p. 2)](zotero://open-pdf/library/items/I9ZRLMUH?page=2&annotation=)  
> *“• Rust 的有限堆栈语义可保证堆栈内存安全和溢出弹性，通过编译时检测和操作系统支持实现。 • 软锁，一种新颖的同步原语，可在具有线程任务的基于Rust 的系统上实现零延迟中断处理。 • Hopter 嵌入式操作系统[25]的开源实现，它集成了FS 语义和软锁，以提供内存安全性、故障恢复能力和响应能力，同时需要最少的应用程序合作。 • 评估 FS 语义、展开机制和软锁产生的代码大小和性能开销，显示 Hopter 对于资源受限的微控制器和实时工作负载的适用性**  


对 soft-lock 最感兴趣，是如何实现零延迟中断的呢




- *“The CPU typically executes instructions directly from the byte-addressable flash, called execute in place (XIP), while function call stacks and mutable data reside in SRAM.”* [(Ma 等, 2025, p. 2)](zotero://open-pdf/library/items/I9ZRLMUH?page=2&annotation=)  
> *“CPU 通常直接从字节可寻址闪存执行指令，称为就地执行 (XIP)，而函数调用堆栈和可变数据驻留在 SRAM 中。**  


不会加载程序到内存中，全局变量似乎也是在 flash 中，具体细节不太确定




- *“MPU/PMP allows developers to define up to 16 memory regions with selective read, write, or execute permissions.”* [(Ma 等, 2025, p. 2)](zotero://open-pdf/library/items/I9ZRLMUH?page=2&annotation=)  
> *“MPU/PMP 允许开发人员定义最多 16 个具有选择性读取、写入或执行权限的内存区域。**  


高端一些的芯片提供了 MPU，用于内存保护，但是会造成一些内存浪费。这个内存保护应该是只地址空间的保护，比如任意地址你都可以保护，哪怕不是 sram 或者 flash。还有就是这种芯片内部 flash 和内存是完全一样的，可以直接寻址访问




- *“Moreover, employing an MPU or PMP requires system calls to switch privilege modes via software interrupts, and arguments need to undergo marshaling and be verified by the OS,”* [(Ma 等, 2025, p. 2)](zotero://open-pdf/library/items/I9ZRLMUH?page=2&annotation=)  
> *“此外，采用MPU或PMP需要系统调用通过软件中断来切换特权模式，并且参数需要经过操作系统的编组和验证，**  


这个啥意思，这种芯片不都是直接运行在 M 级别吗


大多数 Cortex-M 上跑的 RTOS：确实“几乎不用 SVC”，task 更像是内核线程。但一旦你“真的启用了 MPU 做隔离”，SVC 就不可避免。


我理解可能是有一种介于 M 和 A 系列之间的芯片，提供了简单的内存保护，但本质还是简单的 rtos。但是这样涉及到特权级切换，会影响性能，也许影响较小，算是一种兼顾性能和安全性的折中




- *“Consequently, the call stack is likely to reach its maximum depth while calling drop handlers,”* [(Ma 等, 2025, p. 2)](zotero://open-pdf/library/items/I9ZRLMUH?page=2&annotation=)  
> *“因此，调用堆栈在调用 drop 处理程序时可能会达到其最大深度，**  


drop 调用比较多，可能会一直调用，造成栈溢出？




- *“Rust complements its compile-time check with a language exception mechanism called panic to forestall memory errors that are detectable only at runtime, such as out-of-bounds array access”* [(Ma 等, 2025, p. 3)](zotero://open-pdf/library/items/I9ZRLMUH?page=3&annotation=)  
> *“Rust 通过一种名为“panic”的语言异常机制来补充其编译时检查，以防止仅在运行时才能检测到的内存错误，例如越界数组访问**  

- *“A panic terminates the normal execution flow of a Rust thread. On resourceful systems such as personal computers, a panic is usually followed by a stack unwinding procedure that iterates through the function frames in the call stack and invokes the drop handlers of live objects to reclaim resources.”* [(Ma 等, 2025, p. 3)](zotero://open-pdf/library/items/I9ZRLMUH?page=3&annotation=)  
> *“恐慌会终止 Rust 线​​程的正常执行流程。在诸如个人计算机之类的资源丰富的系统上，恐慌之后通常会出现一个堆栈展开过程，该过程会迭代调用堆栈中的函数帧并调用活动对象的删除处理程序以回收资源。**  


通过 panic 机制提供更方便的错误处理

- *“However, embedded Rust systems usually lack a stack unwinder [11, 12, 41] and as a result, a panic will hang or reset the system.”* [(Ma 等, 2025, p. 3)](zotero://open-pdf/library/items/I9ZRLMUH?page=3&annotation=)  
> *“然而，嵌入式 Rust 系统通常缺乏堆栈展开器 [11,12,41]，因此，恐慌会挂起或重置系统。**  


这几篇文章可以看一下




- *“Worse, assertions are common in embedded Rust code [30, 41]. For system robustness, Hopter incorporates a stack unwinder optimized for microcontrollers (§4.2).”* [(Ma 等, 2025, p. 3)](zotero://open-pdf/library/items/I9ZRLMUH?page=3&annotation=)  
> *“更糟糕的是，断言在嵌入式 Rust 代码中很常见 [30, 41]。为了系统的稳健性，Hopter 采用了针对微控制器优化的堆栈开卷机（第 4.2 节）。**  


确实，如果 assert 失败然后 panic 导致系统卡死，也很傻，assert 完全没起到应该的作用




- *“Hopter addresses this problem by running Rust code with finite-stack semantics (§4.1), which not only prevents stack overflows but also converts such errors into Rust panics to allow a unified recovery procedure through unwinding and restarting.”* [(Ma 等, 2025, p. 3)](zotero://open-pdf/library/items/I9ZRLMUH?page=3&annotation=)  
> *“Hopter 通过使用有限堆栈语义运行 Rust 代码（第 4.1 节）来解决这个问题，这不仅可以防止堆栈溢出，还可以将此类错误转换为 Rust 恐慌，以允许通过展开和重新启动进行统一的恢复过程。**  


也就是说，不会导致编译失败，而是将其改写？避免其他组件做修改




- *“Hopter expects application code to be benign, because application code may use unsafe Rust that potentially introduces memory errors. Hopter guarantees memory safety of the system as long as all unsafe code used by the application is sound.”* [(Ma 等, 2025, p. 3)](zotero://open-pdf/library/items/I9ZRLMUH?page=3&annotation=)  
> *“Hopter 希望应用程序代码是良性的，因为应用程序代码可能使用不安全的 Rust，这可能会引入内存错误。只要应用程序使用的所有不安全代码都是健全的，Hopter 就能保证系统的内存安全。**  


如果你使用了 unsafe，那就要对自己负责，所以本质上还是一种软件上，以来 rust 语言的内存安全设计，没有办法限制非安全库，当然这也是因为在资源受限系统上




- *“Hopter’s threat model is weaker than that of Tock [41] where application code can be malicious and the OS must isolate itself using hardware-based protection (See §2), along with its overhead.”* [(Ma 等, 2025, p. 3)](zotero://open-pdf/library/items/I9ZRLMUH?page=3&annotation=)  
> *“Hopter 的威胁模型比 Tock [41] 的威胁模型更弱，其中应用程序代码可能是恶意的，操作系统必须使用基于硬件的保护（参见第 2 节）及其开销来隔离自身。**  


tock 可以防止 app 恶意代码，但是 hopter 不行，但是




- *“Upon fatal errors like stack overflows, out-of-bounds array accesses, and failed assertions, restartable tasks terminate, release their resources, and automatically restart execution from beginning.”* [(Ma 等, 2025, p. 4)](zotero://open-pdf/library/items/I9ZRLMUH?page=4&annotation=)  
> *“当出现堆栈溢出、越界数组访问和断言失败等致命错误时，可重新启动的任务将终止，释放其资源，并自动从头开始重新执行。**  


是针对这些问题 panic，可以不影响系统重启任务，而不是说所有 panic 都可以解决，比如你用 unsafe 恶意修改某个寄存器，那肯定是不行的




- *“Hopter cannot recover from interrupt stack overflows, due to the restrictions of dynamic memory allocation within the interrupt context.”* [(Ma 等, 2025, p. 4)](zotero://open-pdf/library/items/I9ZRLMUH?page=4&annotation=)  
> *“由于中断上下文中动态内存分配的限制，Hopter 无法从中断堆栈溢出中恢复。**  


中断发生的时候是无法恢复的，只能停止回到其他 app，或者如果 mask 没有被清除，会重新执行


通常系统中中断是一个堆栈，可以理解为一个控制流




- *“They allow synchronization between interrupt handlers and tasks without compromising the zero-latency response time to interrupts.”* [(Ma 等, 2025, p. 4)](zotero://open-pdf/library/items/I9ZRLMUH?page=4&annotation=)  
> *“它们允许中断处理程序和任务之间的同步，而不会影响中断的零延迟响应时间。**  


个人感觉比较重要，要看下怎么实现




- *“Hopter employs a compiler-based mechanism to guarantee memory safety for restartable tasks and fallible interrupt handlers.”* [(Ma 等, 2025, p. 4)](zotero://open-pdf/library/items/I9ZRLMUH?page=4&annotation=)  
> *“Hopter 采用基于编译器的机制来保证可重新启动任务和易出错的中断处理程序的内存安全。**  


利用 rust，确保内存相关的安全（对于软件来说，内存问题基本是最大的问题），或者说，异常处理做的更好？但是硬件问题，或者说不进入 panic 的问题，是没办法处理的

- *“FS-semantics unifies runtime memory errors and other fatal errors as Rust panics, allowing Hopter to apply a universal recovery mechanism based on unwinding and restarting”* [(Ma 等, 2025, p. 4)](zotero://open-pdf/library/items/I9ZRLMUH?page=4&annotation=)  
> *“FS 语义将运行时内存错误和其他致命错误统一为 Rust 恐慌，允许 Hopter 应用基于展开和重新启动的通用恢复机制**  


解释了上面的问题，出现软件 panic 的时候可以才保证系统安全，重启任务。通过 unwind 栈找到当前任务执行的点，从而可以继续执行




- *“This is achieved by a prologue of instructions emitted by the compiler before the function body that allocates a stack frame.”* [(Ma 等, 2025, p. 4)](zotero://open-pdf/library/items/I9ZRLMUH?page=4&annotation=)  
> *“这是通过编译器在分配堆栈帧的函数体前发出指令的序言来实现的。**  


函数调用前栈指针下移的指令之前增加一个检查指令

- *“The prologue computes the free stack size as the difference between the current stack top indicated by the stack pointer and the stack region boundary stored in a task-local variable BOUNDARY.”* [(Ma 等, 2025, p. 4)](zotero://open-pdf/library/items/I9ZRLMUH?page=4&annotation=)  
> *“前言将空闲栈大小计算为栈指针指示的当前栈顶与存储在任务局部变量BOUNDARY中的栈区域边界之差。**  


该前言指令检查栈空间是否还足够




- *“If there is insufficient free space, it traps into the OS for further diagnosis.”* [(Ma 等, 2025, p. 4)](zotero://open-pdf/library/items/I9ZRLMUH?page=4&annotation=)  
> *“如果没有足够的自由空间，它就会进入OS进行进一步的诊断。**  


这个 trap 进 OS 没太理解，M 系列上的系统需要 trap 吗，还是说这个 trap 只是指函数调用，或者触发异常。实现了 rust 的 panic，进入处理函数，就算 trap 吗




- *“After trapping, Hopter sets the program counter of the task to the stack unwinder entry to start the unwinding.”* [(Ma 等, 2025, p. 4)](zotero://open-pdf/library/items/I9ZRLMUH?page=4&annotation=)  
> *“陷阱捕获后，Hopter将任务的程序计数器设置到堆栈解卷器入口开始解卷。**  


所以是有一个异常处理函数？并且执行 unwind 过程




- *“This is because stack unwinding must not start inside a drop handler, as doing so would skip some code responsible for releasing resources.”* [(Ma 等, 2025, p. 4)](zotero://open-pdf/library/items/I9ZRLMUH?page=4&annotation=)  
> *“这是因为堆栈解卷不能在下拉处理程序内部启动，因为这样做会跳过一些负责释放资源的代码。**  


unwind 程序不能在 drop handler 中使用，是不是 unwind 本身就是在不停的调用 drop？如果像了解详细原因，得看下 unwind 是怎么实现并工作的




- *“Whenever a drop handler starts executing, it sets the IN_DROP task-local variable to true”* [(Ma 等, 2025, p. 5)](zotero://open-pdf/library/items/I9ZRLMUH?page=5&annotation=)  
> *“当Drop句柄开始执行时，设置IN _ DROP任务-局部变量为真。**  


这个是 hopter 实现的吗，还是 rust 就带这个，还有就是 m 系列的芯片上 tls 的实现和 A 系列相同吗




- *“Note that the function prologue is still applied on top of the instrumentation for drop handlers.”* [(Ma 等, 2025, p. 5)](zotero://open-pdf/library/items/I9ZRLMUH?page=5&annotation=)  
> *“值得注意的是，函数序言仍然应用于对丢弃处理器的插桩之上。**  


即使是 drop handler 执行前也会检查，不过这很细节了，目前不需要了解




- *“The LLVM compiler backend infers the nounwind attribute for functions and simplifies generated code based on it.”* [(Ma 等, 2025, p. 5)](zotero://open-pdf/library/items/I9ZRLMUH?page=5&annotation=)  
> *“LLVM编译器后端对函数进行名风属性推断，并在此基础上对生成的代码进行简化。**  


看起来 unwind 是一种很常见的机制，不知道 qnx 上是否有类似故障处理应用。查了下，qnx 中也会使用 libunwind 作为 backtrace，应用程序




- *“A segmented stack is a linked list of non-contiguous memory chunks called stacklets.”* [(Ma 等, 2025, p. 5)](zotero://open-pdf/library/items/I9ZRLMUH?page=5&annotation=)  
> *“分段堆栈是由非连续的内存块组成的链表，称为堆栈小块( stacklets )。**  


这之前没有了解过，栈可以这样使用，但是要在编译器里实现吧。这样分配和回收效率算法复杂度低，链表实现的




- *“In contrast, hardwarebased protection typically detects the overflow after the function body has started execution and accessed an invalid memory address.”* [(Ma 等, 2025, p. 5)](zotero://open-pdf/library/items/I9ZRLMUH?page=5&annotation=)  
> *“相比之下，基于硬件的保护通常在功能体开始执行并访问无效内存地址后检测溢出。**  


这是本方法的优势，如果等到硬件报错，那现场可能已经被破坏了




- *“A panic occurs due to an out-of-bounds array access, a failed assertion, or a stack overflow under FS-semantics.”* [(Ma 等, 2025, p. 5)](zotero://open-pdf/library/items/I9ZRLMUH?page=5&annotation=)  
> *“在FS语义下，由于越界的数组访问、失败的断言或栈溢出而产生恐慌。**  


前两个是 rust 本身行为，第三个是 hopter 实现的




- *“Thus, interrupt handlers on Hopter are fallible and tasks restartable.”* [(Ma 等, 2025, p. 5)](zotero://open-pdf/library/items/I9ZRLMUH?page=5&annotation=)  
> *“因此，Hopter上的中断处理程序是错误的，任务是可重新起动的的。**  


中断错误后就退出了，任务错误后还可以恢复




- *“The unwinding procedure runs in the context of the panicked task or interrupt handler, during which the scheduler can continue to perform context switches and higher-priority interrupts can nest atop”* [(Ma 等, 2025, p. 5)](zotero://open-pdf/library/items/I9ZRLMUH?page=5&annotation=)  
> *“解卷过程在惊慌失措的任务或中断处理程序的上下文中运行，在此期间调度器可以继续执行上下文切换，更高优先级的中断可以嵌套在上面。**  


这里的细节有点疑惑，unwind 过程是执行在发生 panic 的任务中吗，但是上面有提到 panic 执行是独立于这些上下文的，应该是在当前上下文吧，否则怎么进行 unwind 呢




- *“Second, Hopter’s unwinder recognizes stacklet boundaries and frees stacklets during unwinding to prevent memory leaks.”* [(Ma 等, 2025, p. 5)](zotero://open-pdf/library/items/I9ZRLMUH?page=5&annotation=)  
> *“其次，Hopter的解卷器识别堆栈边界，并在解卷时释放堆栈，以防止内存泄漏。**  


在 unwind 的过程中，涉及一些 drop 过程，同时会回收栈块







- *“Hopter requires the task’s entry closure to implement the Clone trait so that it can safely duplicate the closure’s enclosed environment.”* [(Ma 等, 2025, p. 5)](zotero://open-pdf/library/items/I9ZRLMUH?page=5&annotation=)  
> *“Hopter要求任务的入口关闭以实现Clone特性，使其能够安全地复制关闭的封闭环境。**  


实际就是，重新启动时，会保留全局变量，所以带入闭包环境中的实例需要实现 clone，这样可以直接拷贝。而局部变量会被销毁，所以不需要实现




- *“Rust’s ownership model eliminates race conditions between the restarted instance and the one being unwound.”* [(Ma 等, 2025, p. 5)](zotero://open-pdf/library/items/I9ZRLMUH?page=5&annotation=)  
> *“Rust的所有权模型消除了重启实例与未被重启实例之间的竞争条件。**  


因为是拷贝了一份？所以完全没有竞争


看了下 git 中的例子，似乎不是从头执行，局部变量也是继承的




- *“Therefore, deferred operations can run without conflict immediately after the code with full access finishes its logic.”* [(Ma 等, 2025, p. 6)](zotero://open-pdf/library/items/I9ZRLMUH?page=6&annotation=)  
> *“因此，在完全访问的代码完成其逻辑后，延迟操作可以立即无冲突地运行。**  


感觉像是记录了使用顺序？通过这种设计可以不害怕有锁的时候被抢占，导致死锁，不需要临界区，因为没有死锁了




- *“An interrupt handler receives partial access to record its intended operations when a preempted context holds the full access. These operations are deferred until the handler returns and the code with full access completes its own operation.”* [(Ma 等, 2025, p. 6)](zotero://open-pdf/library/items/I9ZRLMUH?page=6&annotation=)  
> *“当一个中断处理程序被抢占的上下文保持完全访问时，中断处理程序接收部分访问以记录其预定的操作。这些操作被推迟，直到处理程序返回，具有完全访问权限的代码完成自己的操作。**  


或者说，这些操作被记录，而不是卡在中断中，等到被抢占程序执行完前序操作后，再执行这些操作。但是这样是不是存在一个问题，如果有依赖这些操作的指令呢，比如加减，那也延迟执行吗，这样是不是太复杂了




- *“Code gets full access if the soft-lock is not already acquired or otherwise partial access.”* [(Ma 等, 2025, p. 6)](zotero://open-pdf/library/items/I9ZRLMUH?page=6&annotation=)  
> *“如果软锁尚未被获取或部分被获取，则代码获得完全访问。**  


第一次获取锁时就和之前一样，所有访问，第二个尝试获取的，只有部分权限




- *“To further prevent race conditions arising from concurrent task execution, the scheduler is always suspended before acquiring a soft-lock, thus code running within task contexts always receives full access.”* [(Ma 等, 2025, p. 6)](zotero://open-pdf/library/items/I9ZRLMUH?page=6&annotation=)  
> *“为了进一步防止并发任务执行时产生的竞争条件，调度器总是在获得软锁之前被暂停，因此在任务上下文中运行的代码总是得到充分的访问。**  


那如果两个任务都要获取一个锁呢，还是说在当前任务释放之前，不会调度到其他任务，这样就简化为中断和任务之间了




- *“When a protected data structure is under contention, the handler uses partial access to record its intended operations, which are deferred for later execution.”* [(Ma 等, 2025, p. 6)](zotero://open-pdf/library/items/I9ZRLMUH?page=6&annotation=)  
> *“当受保护的数据结构受到竞争时，处理程序使用部分访问来记录其预定的操作，这些操作被延迟到以后的执行中。**  


所谓的 partial access 不是说访问结构体中的一部分，而是指它受限，必须等 full 释放后，才能执行它。但是执行的后续是什么呢，是锁之后的所有代码吗，如果不是，那锁的意义呢。这块有点模糊，得看具体代码实现




- *“while the Interrupt-task column reports that observed when the handler notifies a high-priority task to respond.”* [(Ma 等, 2025, p. 9)](zotero://open-pdf/library/items/I9ZRLMUH?page=9&annotation=)  
> *“而Interrupt - task列报告了当处理程序通知高优先级任务进行响应时观察到的情况。**  


可以理解为同步？，中断处理函数通知 task 进行处理或者响应时的延迟，或者说是从中断到 task 切换的延时，总的来说 rust 都要比 c 高




- *“Otherwise, the Tock detects the fault and restarts the task, but the lock will not be released, subsequently causing a deadlock.”* [(Ma 等, 2025, p. 10)](zotero://open-pdf/library/items/I9ZRLMUH?page=10&annotation=)  
> *“否则，Tock检测到故障并重新启动任务，但锁不会被释放，从而导致死锁。**  


tock 缺少 unwinder 的处理过程，导致锁没有释放，没有清理干净

- *“Since FreeRTOS has no safety protection, the out-of-bounds write either causes a system-wide data corruption, or triggers a hardware fault that hangs or resets the system.”* [(Ma 等, 2025, p. 10)](zotero://open-pdf/library/items/I9ZRLMUH?page=10&annotation=)  
> *“由于FreeRTOS没有安全保护，越界写入要么导致系统范围内的数据损坏，要么触发硬件故障导致系统挂起或重置。**  


freertos 是完全没有保护的




- *“The drone rotates around 20 degrees along the yaw axis and drops a few centimeters when the Stabilizer task panics, but can still hover after the task restart.”* [(Ma 等, 2025, p. 11)](zotero://open-pdf/library/items/I9ZRLMUH?page=11&annotation=)  
> *“当稳定器任务发生恐慌时，无人机会沿着偏航轴旋转约 20 度并下降几厘米，但在任务重新启动后仍然可以悬停。**  


最关键的任务 panic 重启后，仍然可以稳定住，重启非常快




- *“Also, Hopter’s soft-lock can improve performance by deferring operations only upon contention, which is rare, thus reducing CPU load by mostly taking the fast path.”* [(Ma 等, 2025, p. 12)](zotero://open-pdf/library/items/I9ZRLMUH?page=12&annotation=)  
> *“此外，Hopter 的软锁可以通过仅在发生争用时推迟操作来提高性能（这种情况很少见），从而通过主要采用快速路径来减少 CPU 负载。**  


不是直接分为上下部分，而是通过在有锁是推迟后续操作，不需要同步的时候，不需要管。目前常用系统中，获取锁的时候，如果感觉和中断相关，通常需要关中断





1️⃣ Hopter 对 Rust 的修改



Hop­ter 主要做了两类修改：


(1) 有限栈语义（finite-stack semantics）


Rust 原生在嵌入式 MCU 上 无法可靠检测栈溢出，尤其是中断上下文和小栈任务。


Hop­ter 修改了 Rust 编译器/运行时，让每次函数调用先检查剩余栈空间：




如果剩余栈空间不足 → 触发 panic，而不是野指针/覆盖其他内存




这其实是运行时安全检查，不是编译时错误检查。



(2) panic 处理与任务重启


Rust 默认 panic 会 unwind 栈 或 abort。


Hop­ter 修改 panic runtime，让 panic 可以：




清理局部资源


通知内核调度器


重启任务




目的不是避免错误，而是系统级鲁棒性。





1️⃣ Rust panic 是核心





Hop­ter 是用 Rust 重写嵌入式 RTOS 的，所以它把所有 任务级错误、运行时安全检查 都交给 Rust 的 panic 机制处理。


包括：




数组越界（arr[i]）


栈溢出（有限栈语义检测）


显式调用 panic!()




Rust panic 可以 栈展开（unwind）或 abort，Hop­ter 在 panic runtime 上做了修改，实现 任务终止 + 重启。


还没到触发 cpu 异常的时候，就在运行时检查到并且触发 rust panic 了。所以分为两步




检查到错误，触发 panic


在 panic 中使用 unwind 实现任务重启


3️⃣ Hopter 的核心原则：三条“铁律”


🔒 铁律 1：内核从不关中断
这是最重要的一条：




没有 disable_irq()


没有“关中断保护内核数据结构”


没有“IRQ-off critical section”


👉 中断随时可进



🧵 铁律 2：任务 ≠ 线程（不是内核线程）
Hopter 的 task：




不共享内核状态


没有传统的“runqueue + scheduler lock”


每个 task 的生命周期、状态是 局部可恢复的


这给了后面一条铁律空间。



🧠 铁律 3：用 soft-locks 代替关中断
这是 Hopter 的关键创新。



4️⃣ Soft-locks 是什么（重点）
soft-lock ≠ mutex ≠ spinlock


它的核心思想是：



允许中断打断，但保证“被打断的代码一定能安全恢复”



举个直觉例子
传统做法：


disable_irq();
shared_state = new_value;
enable_irq();

Hopter 的思路是：


let guard = soft_lock.enter();
// 修改共享状态
// 中断可以随时发生
drop(guard);

如果在中途被 IRQ 打断：




IRQ 不会破坏一致性


被打断的代码 一定能继续或安全失败（panic）


panic → unwind → 任务重启


👉 正确性不是靠“不被打断”保证的，而是靠“可恢复性”保证的






- *“Table 3: Binary size, SRAM usage, and CPU load of the flight control system. The SRAM usage excludes function call stacks, whose sizes are configurable. The CPU load is the average of 100 measurements when the drone is set to hover.”* [(Ma 等, 2025, p. 10)](zotero://open-pdf/library/items/I9ZRLMUH?page=10&annotation=)  
> *“表 3：飞控系统的二进制大小、SRAM 使用情况和 CPU 负载。 SRAM 的使用不包括函数调用堆栈，其大小是可配置的。 CPU 负载是无人机悬停时 100 次测量的平均值。**  


CPU 利用率还是增加了很多的


