#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

#[no_mangle]
pub static STATE: AtomicUsize = AtomicUsize::new(0);

struct DropTest;
impl Drop for DropTest {
    fn drop(&mut self) {
        STATE.store(2, Ordering::SeqCst);
    }
}

#[panic_handler]
fn panic_handler(_info: &PanicInfo) -> ! {
    STATE.store(100, Ordering::SeqCst);
    loop {}
}

const STACK_SIZE: usize = 256 * 1024;

#[no_mangle]
#[link_section = ".text.entry"]
pub unsafe extern "C" fn _start() -> ! {
    let stack_top = 0x40000000u64 + (128 * 1024 * 1024) as u64 - STACK_SIZE as u64;
    asm!("mov sp, {}", in(reg) stack_top);
    main()
}

#[no_mangle]
pub extern "C" fn main() -> ! {
    STATE.store(1, Ordering::SeqCst);
    
    {
        let _drop_test = DropTest;
        panic!("test panic");
    }
    
    unreachable!()
}
