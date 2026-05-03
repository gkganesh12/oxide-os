pub mod scheduler;

use crate::println;

pub fn init() {
    scheduler::init();
    println!("[gpu] GPU subsystem initialized");
}
