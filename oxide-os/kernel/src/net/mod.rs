pub mod virtio_net;
pub mod stack;
pub mod socket;
pub mod dns;
pub mod http;
pub mod firewall;

use crate::println;

pub fn init(hhdm_offset: u64) {
    virtio_net::init(hhdm_offset);
    stack::init();
    dns::init();
    println!("[net] Network subsystem initialized");
}
