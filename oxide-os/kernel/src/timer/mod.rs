pub mod clock;
pub mod deadline;

use crate::println;

pub fn init() {
    clock::init();
    deadline::init();
    println!("[timer] Timer subsystem initialized");
}
