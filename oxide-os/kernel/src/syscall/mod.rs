pub mod numbers;
pub mod handler;

use crate::println;

pub fn init() {
    println!("[syscall] Syscall interface initialized ({} calls registered)", numbers::SYSCALL_COUNT);
}
