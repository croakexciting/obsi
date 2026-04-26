#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

// ─────────────────────────────────────────────────────────────────────────────
// aarch64-baremetal-unwind：在 AArch64 bare-metal 上演示 Hopter 风格的 unwind
//
// 核心设计：
//   用户代码只写 panic!()，完全不知道 recovery 机制。
//   OS 层（task_run）预先注册 landing pad，panic_handler 触发 unwind 过程。
//
//   执行流程：
//     panic!()
//       → #[panic_handler]          ← Rust 运行时自动调用
//       → start_unwind_entry()      ← 相当于 Hopter 的 start_unwind_entry
//       → begin_panic()             ← 启动 unwinding crate 的 .eh_frame unwind
//       → 走帧，每帧调用 Drop        ← 等同于 Hopter 的 unwind_next_function
//       → 找到 task_run 的 landing pad  ← .eh_frame 中 catch_unwind 生成的 landing pad
//       → 跳回 task_run 的恢复代码   ← Hopter 中跳回 task 的 catch block
//       → main 继续运行
// ─────────────────────────────────────────────────────────────────────────────

extern crate alloc;
extern crate unwinding;

use core::arch::asm;
use core::ffi::c_void;
use alloc::boxed::Box;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::panic::PanicInfo;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicUsize, Ordering};
use unwinding::abi::{
    UnwindContext, UnwindException, UnwindReasonCode, _Unwind_Backtrace, _Unwind_GetIP,
};

// Demonstration hook: count and observe every resume-unwind continuation.
static RESUME_WRAP_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C-unwind" {
    fn __real__Unwind_Resume(exception_object: *mut UnwindException) -> !;
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn __wrap__Unwind_Resume(
    exception_object: *mut UnwindException,
) -> ! {
    RESUME_WRAP_COUNT.fetch_add(1, Ordering::SeqCst);

    unsafe { __real__Unwind_Resume(exception_object) }
}

// PL011 UART (QEMU virt 固定地址 0x09000000)
// 用 inline asm 直接 str，绕过 debug 模式的 write_volatile 检查层
fn uart_init() {}

#[inline(always)]
fn uart_byte(b: u8) {
    unsafe {
        asm!(
            "str {b:w}, [{uart}]",
            uart = in(reg) 0x0900_0000usize,
            b = in(reg) b as u32,
            options(nostack),
        );
    }
}

fn uart_print(s: &str) {
    for b in s.bytes() {
        if b == b'\n' { uart_byte(b'\r'); }
        uart_byte(b);
    }
}

fn uart_hex(mut v: usize) {
    uart_print("0x");
    if v == 0 { uart_byte(b'0'); return; }
    let mut buf = [0u8; 16];
    let mut i = 16usize;
    while v > 0 {
        i -= 1;
        let d = (v & 0xf) as u8;
        buf[i] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
        v >>= 4;
    }
    for &b in &buf[i..] { uart_byte(b); }
}

fn uart_usize(mut v: usize) {
    if v == 0 { uart_byte(b'0'); return; }
    let mut buf = [0u8; 20];
    let mut i = 20usize;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for &b in &buf[i..] { uart_byte(b); }
}

// ── 全局分配器 (unwinding crate 需要 alloc) ──────────
const HEAP_SIZE: usize = 256 * 1024;

#[repr(align(16))]
struct HeapStorage([u8; HEAP_SIZE]);

struct BumpAllocator {
    heap: UnsafeCell<HeapStorage>,
    next: AtomicUsize,
}
unsafe impl Sync for BumpAllocator {}

impl BumpAllocator {
    const fn new() -> Self {
        Self {
            heap: UnsafeCell::new(HeapStorage([0; HEAP_SIZE])),
            next: AtomicUsize::new(0),
        }
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut cur = self.next.load(Ordering::Relaxed);
        loop {
            let aligned = (cur + layout.align() - 1) & !(layout.align() - 1);
            let end = match aligned.checked_add(layout.size()) {
                Some(e) => e,
                None => return null_mut(),
            };
            if end > HEAP_SIZE { return null_mut(); }
            match self.next.compare_exchange(cur, end, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => {
                    let heap = unsafe { &mut *self.heap.get() };
                    return unsafe { heap.0.as_mut_ptr().add(aligned) };
                }
                Err(observed) => cur = observed,
            }
        }
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator::new();

#[alloc_error_handler]
fn oom(_: Layout) -> ! {
    uart_print("[FATAL] out of memory\n");
    loop { core::hint::spin_loop(); }
}

// ── Backtrace ─────────────────────────────────────────
extern "C" fn backtrace_cb(ctx: &UnwindContext<'_>, arg: *mut c_void) -> UnwindReasonCode {
    let n = unsafe { &mut *(arg as *mut usize) };
    let ip = unsafe { _Unwind_GetIP(ctx) };
    uart_print("  #");
    uart_usize(*n);
    uart_print("  ");
    uart_hex(ip);
    uart_print("\n");
    *n += 1;
    UnwindReasonCode::NO_REASON
}

fn print_backtrace() {
    uart_print("stack backtrace:\n");
    let mut n: usize = 0;
    unsafe { _Unwind_Backtrace(backtrace_cb, &mut n as *mut usize as *mut c_void); }
}

// ─────────────────────────────────────────────────────────────────────────────
// panic handler：panic 发生时由 Rust 运行时自动调用
// 相当于 Hopter 的 #[panic_handler] unsafe fn panic()
// ─────────────────────────────────────────────────────────────────────────────
#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    uart_print("\n[panic_handler] panic at ");
    if let Some(loc) = info.location() {
        uart_print(loc.file());
        uart_print(":");
        uart_usize(loc.line() as usize);
    }
    uart_print("\n");
    uart_print("[panic_handler] calling start_unwind_entry()...\n\n");

    start_unwind_entry()
}

// ─────────────────────────────────────────────────────────────────────────────
// start_unwind_entry：启动 unwind 过程
//
// 相当于 Hopter 的 start_unwind_entry()。
// 调用 begin_panic，它会：
//   1. 从当前帧开始，沿调用链向上走
//   2. 对每个有 cleanup landing pad 的帧，调用 personality routine
//   3. personality routine 根据 .eh_frame 中的 LSDA 信息，执行 Drop（cleanup）
//   4. 直到找到 catch_unwind 对应的 landing pad（在 task_run 里）
//   5. 跳回 task_run 的恢复代码
//
// 注意：这个函数永远不会返回，执行流会直接跳到 landing pad
// ─────────────────────────────────────────────────────────────────────────────
fn start_unwind_entry() -> ! {
    uart_print("[start_unwind_entry] initiating .eh_frame walk...\n");

    // begin_panic 启动 Itanium C++ ABI / DWARF unwind：
    //   内部调用 _Unwind_RaiseException，从当前帧向上逐帧：
    //   - 第一遍（search phase）：找 catch_unwind landing pad
    //   - 第二遍（cleanup phase）：执行沿途每帧的 Drop cleanup
    let _ = unwinding::panic::begin_panic(Box::new(()));

    // 不应该到达这里，begin_panic 一定跳走了
    uart_print("[start_unwind_entry] UNREACHABLE: no landing pad found, halting\n");
    loop { core::hint::spin_loop(); }
}

// ─────────────────────────────────────────────────────────────────────────────
// OS 层：task_run
//
// 相当于 Hopter 的任务执行框架，内部注册 landing pad（catch_unwind），
// 但用户代码完全看不到它，只是运行一个普通函数。
//
// 这里的 catch_unwind 并不是由用户调用的，而是 OS 基础设施的一部分。
// 编译器会在 .eh_frame 中为 catch_unwind 生成 landing pad 信息，
// unwind 过程通过 .eh_frame 找到它，然后跳回这里。
// ─────────────────────────────────────────────────────────────────────────────
fn task_run(task_fn: fn()) {
    uart_print("[task_run] OS: registering landing pad and starting task...\n\n");

    // catch_unwind 由 OS 层注册，用户代码不感知
    // 等价于 Hopter 在 task 入口处设置 catch block
    let result = unwinding::panic::catch_unwind(task_fn);

    // 执行流从这里恢复（从 start_unwind_entry → begin_panic → .eh_frame 找到这里）
    match result {
        Ok(_) => {
            uart_print("\n[task_run] task completed normally\n");
        }
        Err(_) => {
            uart_print("\n[task_run] OS: unwind landed here! task panicked but recovered.\n");
            uart_print("[task_run] all resources in task have been dropped.\n");
            uart_print("[task_run] custom resume_unwind hook count = ");
            uart_usize(RESUME_WRAP_COUNT.load(Ordering::SeqCst));
            uart_print("\n");
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 用户代码：只写业务逻辑，不知道 recovery 机制
// ─────────────────────────────────────────────────────────────────────────────

struct Resource(&'static str);

impl Drop for Resource {
    fn drop(&mut self) {
        // 这里由 unwind 过程（personality routine + LSDA）自动调用
        // 对应 .eh_frame 中的 cleanup landing pad
        uart_print("  [Drop] ");
        uart_print(self.0);
        uart_print("  ← unwind 触发 Drop\n");
    }
}

/// 用户函数 level_c：panic 触发点
fn level_c() {
    let _r = Resource("C::file_handle");
    uart_print("  [level_c] 触发 panic!\n");
    panic!("数据损坏");  // 用户只写 panic!，不知道后面发生了什么
}

/// 用户函数 level_b
fn level_b() {
    let _r = Resource("B::connection");
    level_c();
}

/// 用户函数 level_a（任务入口，被 task_run 调用）
fn level_a() {
    let _r1 = Resource("A::mutex_guard");
    let _r2 = Resource("A::vec_data");
    uart_print("  [level_a] 开始执行，分配资源...\n");
    level_b();
    uart_print("  [level_a] 这行不应该被执行\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// 裸机入口
// ─────────────────────────────────────────────────────────────────────────────
#[no_mangle]
pub extern "C" fn main() -> ! {
    uart_init();
    uart_print("╔══════════════════════════════════════════════════╗\n");
    uart_print("║  AArch64 bare-metal Hopter 风格 unwind 演示       ║\n");
    uart_print("╠══════════════════════════════════════════════════╣\n");
    uart_print("║  panic!() → panic_handler → start_unwind_entry  ║\n");
    uart_print("║  → begin_panic → .eh_frame 走帧 → Drop           ║\n");
    uart_print("║  → task_run landing pad → 恢复执行流              ║\n");
    uart_print("╚══════════════════════════════════════════════════╝\n\n");

    // OS 层：运行任务，用户代码无需了解 recovery 机制
    task_run(level_a);

    uart_print("\n[main] task_run 返回，main 继续运行，系统未崩溃。\n");
    uart_print("[main] done.\n");
    loop { core::hint::spin_loop(); }
}

// STACK_TOP = 0x40000000 (RAM base) + 128*1024*1024 (128MB) - 256*1024 (stack) = 0x47FC0000
// 初始化步骤：
//   1. 使能 FP/NEON (CPACR_EL1.FPEN = 0b11, bits[21:20])，否则 SIMD 指令 trap
//   2. 设置栈指针
//   3. 跳转 main
core::arch::global_asm!(
    ".section .text.entry, \"ax\"",
    ".global _start",
    "_start:",
    "    mov x0, #(3 << 20)",   // FPEN: enable FP/SIMD for EL0 and EL1
    "    msr cpacr_el1, x0",
    "    isb",
    "    ldr x0, =0x47FC0000",
    "    mov sp, x0",
    "    bl main",
    "1:  b 1b",
);
