//! PCI bus enumeration via x86 port I/O (configuration mechanism #1).
//!
//! Scans all bus/device/function combinations to discover PCI devices.
//! Used by virtio drivers to find their hardware.

use x86_64::instructions::port::Port;
use spin::Mutex;
use alloc::vec::Vec;
use crate::println;

const PCI_CONFIG_ADDR: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub header_type: u8,
    pub bars: [u32; 6],
    pub interrupt_line: u8,
}

/// Read a 32-bit value from PCI configuration space.
fn pci_config_read(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address: u32 = (1 << 31) // Enable bit
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);

    unsafe {
        Port::<u32>::new(PCI_CONFIG_ADDR).write(address);
        Port::<u32>::new(PCI_CONFIG_DATA).read()
    }
}

/// Write a 32-bit value to PCI configuration space.
pub fn pci_config_write(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let address: u32 = (1 << 31)
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);

    unsafe {
        Port::<u32>::new(PCI_CONFIG_ADDR).write(address);
        Port::<u32>::new(PCI_CONFIG_DATA).write(value);
    }
}

/// Read all 6 BARs for a device.
fn read_bars(bus: u8, device: u8, function: u8) -> [u32; 6] {
    let mut bars = [0u32; 6];
    for i in 0..6 {
        bars[i] = pci_config_read(bus, device, function, 0x10 + (i as u8) * 4);
    }
    bars
}

/// Scan the PCI bus and return all discovered devices.
pub fn scan() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    for bus in 0..=255u8 {
        for device in 0..32u8 {
            let vendor_device = pci_config_read(bus, device, 0, 0);
            let vendor_id = (vendor_device & 0xFFFF) as u16;
            if vendor_id == 0xFFFF { continue; } // No device

            let device_id = ((vendor_device >> 16) & 0xFFFF) as u16;
            let class_reg = pci_config_read(bus, device, 0, 0x08);
            let class_code = ((class_reg >> 24) & 0xFF) as u8;
            let subclass = ((class_reg >> 16) & 0xFF) as u8;
            let header_reg = pci_config_read(bus, device, 0, 0x0C);
            let header_type = ((header_reg >> 16) & 0xFF) as u8;
            let int_reg = pci_config_read(bus, device, 0, 0x3C);
            let interrupt_line = (int_reg & 0xFF) as u8;

            let bars = read_bars(bus, device, 0);

            devices.push(PciDevice {
                bus, device, function: 0,
                vendor_id, device_id, class_code, subclass,
                header_type, bars, interrupt_line,
            });

            // Check multi-function
            if header_type & 0x80 != 0 {
                for func in 1..8u8 {
                    let vd = pci_config_read(bus, device, func, 0);
                    let vid = (vd & 0xFFFF) as u16;
                    if vid == 0xFFFF { continue; }
                    let did = ((vd >> 16) & 0xFFFF) as u16;
                    let cr = pci_config_read(bus, device, func, 0x08);
                    let ir = pci_config_read(bus, device, func, 0x3C);
                    let fb = read_bars(bus, device, func);
                    devices.push(PciDevice {
                        bus, device, function: func,
                        vendor_id: vid, device_id: did,
                        class_code: ((cr >> 24) & 0xFF) as u8,
                        subclass: ((cr >> 16) & 0xFF) as u8,
                        header_type: 0, bars: fb,
                        interrupt_line: (ir & 0xFF) as u8,
                    });
                }
            }
        }
        if bus == 255 { break; } // Prevent overflow
    }

    devices
}

/// Find a PCI device by vendor and device ID.
pub fn find_device(vendor_id: u16, device_id: u16) -> Option<PciDevice> {
    scan().into_iter().find(|d| d.vendor_id == vendor_id && d.device_id == device_id)
}

/// Find any device by vendor ID.
pub fn find_by_vendor(vendor_id: u16) -> Vec<PciDevice> {
    scan().into_iter().filter(|d| d.vendor_id == vendor_id).collect()
}

/// Enable PCI bus mastering for a device (required for DMA/virtio).
pub fn enable_bus_master(dev: &PciDevice) {
    let cmd = pci_config_read(dev.bus, dev.device, dev.function, 0x04);
    pci_config_write(dev.bus, dev.device, dev.function, 0x04, cmd | 0x04); // Set bit 2
}

pub fn init() {
    let devices = scan();
    println!("[pci] Found {} devices", devices.len());
    for dev in &devices {
        println!("[pci]   {:02X}:{:02X}.{} vendor={:04X} device={:04X} class={:02X}:{:02X}",
            dev.bus, dev.device, dev.function,
            dev.vendor_id, dev.device_id, dev.class_code, dev.subclass);
    }
}
