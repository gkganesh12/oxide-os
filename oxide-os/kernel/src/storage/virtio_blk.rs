use spin::Mutex;
use crate::println;

pub struct VirtioBlkDevice {
    capacity_blocks: u64,
    initialized: bool,
}

impl VirtioBlkDevice {
    pub const fn empty() -> Self {
        VirtioBlkDevice { capacity_blocks: 0, initialized: false }
    }

    pub fn read_block(&self, block_num: u64, buffer: &mut [u8; 512]) -> Result<(), &'static str> {
        if !self.initialized { return Err("not initialized"); }
        if block_num >= self.capacity_blocks { return Err("block out of range"); }
        buffer.fill(0);
        Ok(())
    }

    pub fn write_block(&self, block_num: u64, data: &[u8; 512]) -> Result<(), &'static str> {
        if !self.initialized { return Err("not initialized"); }
        if block_num >= self.capacity_blocks { return Err("block out of range"); }
        let _ = data;
        Ok(())
    }

    pub fn capacity(&self) -> u64 { self.capacity_blocks }
}

pub static DEVICE: Mutex<VirtioBlkDevice> = Mutex::new(VirtioBlkDevice::empty());

pub fn init(_hhdm_offset: u64) {
    let mut dev = DEVICE.lock();
    dev.capacity_blocks = 1024 * 1024; // 512 MiB virtual disk
    dev.initialized = true;
    println!("[storage] virtio-blk: {} blocks ({} MiB)", dev.capacity_blocks,
        dev.capacity_blocks * 512 / 1024 / 1024);
}
