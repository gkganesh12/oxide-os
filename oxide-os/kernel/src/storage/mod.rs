pub mod virtio_blk;
pub mod block_cache;
pub mod oxide_fs;
pub mod context_store;

use crate::println;

pub const BLOCK_SIZE: u64 = 512;

pub fn init(hhdm_offset: u64) {
    virtio_blk::init(hhdm_offset);
    block_cache::init();
    oxide_fs::init();
    context_store::init();
    println!("[storage] Storage subsystem initialized");
}
