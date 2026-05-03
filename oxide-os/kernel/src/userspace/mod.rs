pub mod elf;
pub mod process;

use crate::println;

pub fn init() {
    println!("[userspace] User-space subsystem initialized");
}
