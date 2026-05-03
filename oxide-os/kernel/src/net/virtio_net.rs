use spin::Mutex;
use crate::println;

pub struct VirtioNetDevice {
    mac: [u8; 6],
    initialized: bool,
}

impl VirtioNetDevice {
    pub const fn empty() -> Self {
        VirtioNetDevice { mac: [0; 6], initialized: false }
    }

    pub fn receive(&mut self, buffer: &mut [u8]) -> Option<usize> {
        if !self.initialized { return None; }
        let _ = buffer;
        None // No packets yet — real driver fills this from virtio ring
    }

    pub fn transmit(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if !self.initialized { return Err("not initialized"); }
        // Real driver would write to virtio TX ring
        let _ = data;
        Ok(())
    }

    pub fn mac_address(&self) -> [u8; 6] { self.mac }
}

pub static DEVICE: Mutex<VirtioNetDevice> = Mutex::new(VirtioNetDevice::empty());

pub fn init(_hhdm_offset: u64) {
    let mut dev = DEVICE.lock();
    dev.mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]; // QEMU default
    dev.initialized = true;
    println!("[net] virtio-net: MAC={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        dev.mac[0], dev.mac[1], dev.mac[2], dev.mac[3], dev.mac[4], dev.mac[5]);
}
