pub mod rng;
pub mod hmac_cap;
pub mod signing;

use crate::println;

pub fn init() {
    rng::init();
    hmac_cap::init();
    signing::init();
    println!("[crypto] Crypto subsystem initialized");
}
