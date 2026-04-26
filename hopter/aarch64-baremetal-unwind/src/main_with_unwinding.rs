#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;
extern crate unwinding;

use core::arch::asm;
use alloc::boxed::Box;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

extern "C" {
    static __stack_end: u8;
}

const HEAP_SIZE: usize = 16 * 1024;
const STACK_SIZE: usize = 256 * 1024;

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
        let align = layout.align();
        let size = layout.size();

        let mut current = self.next.load(Ordering::Relaxed);
        loop {
            let aligned = (current + align - 1) & !(align - 1);
            let end = match aligned.checked_add(size) {
                Some(end) => end,
                None => return null_mut(),
            };

            if end > HEAP_SIZE {
                return null_mut();
            }

            match self.next.compare_exchange(
                current,
                end,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    let heap = unsafe { &mut *self.heap.get() };
                    return unsafe { heap.0.as_mut_ptr().add(aligned) };
                }
                Err(observed) => current = observed,
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator::new();

#[alloc_error_handler]
fn alloc_error(_layout: Layout) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
pub static DEMO_UNWIND_STATE: AtomicUsize = AtomicUsize::new(0);

static PANIC_CAUGHT: AtomicBool = AtomicBool::new(false);

struct DropMarker;

impl Drop for DropMarker {
    fn drop(&mut self) {
        DEMO_UNWIND_STATE.store(2, Ordering::SeqCst);
    }
}

trait InspectCatch {
    fn is_err_and_mark(self) -> bool;
}

impl InspectCatch for Result<(), Box<dyn core::any::Any + Send>> {
    fn is_err_and_mark(self) -> bool {
        if self.is_err() {
            PANIC_CAUGHT.store(true, Ordering::SeqCst);
            DEMO_UNWIND_STATE.store(3, Ordering::SeqCst);
            true
        } else {
            false
        }
    }
}

#[panic_handler]
fn my_panic(info: &PanicInfo) -> ! {
    unsafe { __my_unwind_start(info) }
}

#[no_mangle]
pub unsafe extern "C" fn __my_unwind_start(info: &PanicInfo) -> ! {
    let _ = info;

    let code = unwinding::panic::begin_panic(Box::new(()));
    DEMO_UNWIND_STATE.store(100 + code.0 as usize, Ordering::SeqCst);

    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
pub extern "C" fn main() -> ! {
    let caught = unwinding::panic::catch_unwind(run_demo).is_err_and_mark();

    if caught && PANIC_CAUGHT.load(Ordering::SeqCst) {
        DEMO_UNWIND_STATE.store(4, Ordering::SeqCst);
    } else {
        DEMO_UNWIND_STATE.store(200, Ordering::SeqCst);
    }

    loop {
        core::hint::spin_loop();
    }
}

fn run_demo() {
    DEMO_UNWIND_STATE.store(1, Ordering::SeqCst);

    let _marker = DropMarker;
    let _boxed = Box::new(MaybeUninit::<u64>::new(0x1234_5678_9abc_def0));

    panic!("bare-metal panic!");
}

#[no_mangle]
#[link_section = ".text.entry"]
pub unsafe extern "C" fn _start() -> ! {
    let stack_top = 0x40000000u64 + (128 * 1024 * 1024) as u64 - STACK_SIZE as u64;
    asm!("mov sp, {}", in(reg) stack_top);
    main()
}
