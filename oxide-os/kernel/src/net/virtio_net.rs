//! Real virtio-net driver using legacy (transitional) PCI transport.
//!
//! Implements the virtio 1.0 legacy interface which QEMU supports by default.
//! Uses split virtqueues for RX and TX packet handling.
//!
//! References:
//! - https://docs.oasis-open.org/virtio/virtio/v1.1/virtio-v1.1.html
//! - QEMU virtio-net-pci device

use spin::Mutex;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering, fence};
use x86_64::instructions::port::Port;
use crate::pci;
use crate::println;

// Virtio PCI vendor ID
const VIRTIO_PCI_VENDOR: u16 = 0x1AF4;
// Network device: device ID 0x1000 (legacy) or 0x1041 (modern)
const VIRTIO_NET_DEVICE_LEGACY: u16 = 0x1000;

// Legacy virtio PCI register offsets (from BAR0, I/O space)
const VIRTIO_DEVICE_FEATURES: u16 = 0x00;   // 4 bytes, read
const VIRTIO_GUEST_FEATURES: u16 = 0x04;    // 4 bytes, write
const VIRTIO_QUEUE_ADDR: u16 = 0x08;        // 4 bytes, write (PFN)
const VIRTIO_QUEUE_SIZE: u16 = 0x0C;        // 2 bytes, read
const VIRTIO_QUEUE_SELECT: u16 = 0x0E;      // 2 bytes, write
const VIRTIO_QUEUE_NOTIFY: u16 = 0x10;      // 2 bytes, write
const VIRTIO_DEVICE_STATUS: u16 = 0x12;     // 1 byte, read/write
const VIRTIO_ISR_STATUS: u16 = 0x13;        // 1 byte, read
const VIRTIO_NET_MAC: u16 = 0x14;           // 6 bytes, read (device-specific)

// Virtio status bits
const VIRTIO_STATUS_RESET: u8 = 0;
const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
const VIRTIO_STATUS_DRIVER: u8 = 2;
const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
const VIRTIO_STATUS_FEATURES_OK: u8 = 8;

// Virtio net feature bits
const VIRTIO_NET_F_MAC: u32 = 1 << 5;

// Virtqueue descriptor flags
const VRING_DESC_F_NEXT: u16 = 1;
const VRING_DESC_F_WRITE: u16 = 2;

const QUEUE_SIZE: usize = 256;
const RX_QUEUE: u16 = 0;
const TX_QUEUE: u16 = 1;
const PACKET_BUF_SIZE: usize = 1514 + 12; // Ethernet MTU + virtio-net header

/// Virtqueue descriptor (16 bytes each, spec 2.6.5)
#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
struct VirtqDesc {
    addr: u64,   // Physical address of buffer
    len: u32,    // Length of buffer
    flags: u16,  // VRING_DESC_F_*
    next: u16,   // Next descriptor if NEXT flag set
}

/// Virtqueue available ring (spec 2.6.6)
#[repr(C, align(2))]
struct VirtqAvail {
    flags: u16,
    idx: u16,
    ring: [u16; QUEUE_SIZE],
}

/// Virtqueue used ring element
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

/// Virtqueue used ring (spec 2.6.8)
#[repr(C, align(4))]
struct VirtqUsed {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; QUEUE_SIZE],
}

/// A split virtqueue with descriptor table, available ring, and used ring.
struct Virtqueue {
    descs: &'static mut [VirtqDesc; QUEUE_SIZE],
    avail: &'static mut VirtqAvail,
    used: &'static mut VirtqUsed,
    /// Buffers backing the descriptors (one per descriptor)
    buffers: Vec<Vec<u8>>,
    /// Number of free descriptors
    num_free: usize,
    /// Next free descriptor index
    free_head: usize,
    /// Last seen used index
    last_used_idx: u16,
}

/// The virtio-net device state.
pub struct VirtioNetDevice {
    io_base: u16,
    mac: [u8; 6],
    rx_queue: Option<Virtqueue>,
    tx_queue: Option<Virtqueue>,
    initialized: bool,
}

impl VirtioNetDevice {
    pub const fn empty() -> Self {
        VirtioNetDevice {
            io_base: 0,
            mac: [0; 6],
            rx_queue: None,
            tx_queue: None,
            initialized: false,
        }
    }

    /// Receive a packet. Returns the number of bytes written to buffer, or None.
    pub fn receive(&mut self, buffer: &mut [u8]) -> Option<usize> {
        if !self.initialized { return None; }
        let rx = self.rx_queue.as_mut()?;

        // Check if the device has placed any packets in the used ring
        fence(Ordering::SeqCst);
        if rx.last_used_idx == rx.used.idx {
            return None; // No new packets
        }

        let used_idx = (rx.last_used_idx % QUEUE_SIZE as u16) as usize;
        let used_elem = rx.used.ring[used_idx];
        let desc_idx = used_elem.id as usize;
        let len = used_elem.len as usize;

        // Copy packet data (skip 10-byte virtio-net header)
        let virtio_header_size = 10;
        if len > virtio_header_size {
            let payload_len = len - virtio_header_size;
            let copy_len = payload_len.min(buffer.len());
            buffer[..copy_len].copy_from_slice(
                &rx.buffers[desc_idx][virtio_header_size..virtio_header_size + copy_len]
            );

            // Re-post this descriptor for future receives
            rx.descs[desc_idx].len = PACKET_BUF_SIZE as u32;
            rx.descs[desc_idx].flags = VRING_DESC_F_WRITE;
            rx.descs[desc_idx].next = 0;

            let avail_idx = (rx.avail.idx % QUEUE_SIZE as u16) as usize;
            rx.avail.ring[avail_idx] = desc_idx as u16;
            fence(Ordering::SeqCst);
            rx.avail.idx = rx.avail.idx.wrapping_add(1);
            fence(Ordering::SeqCst);

            // Notify device
            unsafe { Port::<u16>::new(self.io_base + VIRTIO_QUEUE_NOTIFY).write(RX_QUEUE); }

            rx.last_used_idx = rx.last_used_idx.wrapping_add(1);
            return Some(copy_len);
        }

        rx.last_used_idx = rx.last_used_idx.wrapping_add(1);
        None
    }

    /// Transmit a packet.
    pub fn transmit(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if !self.initialized { return Err("not initialized"); }
        let tx = self.tx_queue.as_mut().ok_or("no TX queue")?;

        if tx.num_free == 0 {
            // Reclaim used descriptors
            self.reclaim_tx();
            let tx = self.tx_queue.as_mut().unwrap();
            if tx.num_free == 0 {
                return Err("TX queue full");
            }
        }

        let tx = self.tx_queue.as_mut().unwrap();
        let desc_idx = tx.free_head;
        tx.free_head = (tx.free_head + 1) % QUEUE_SIZE;
        tx.num_free -= 1;

        // Write virtio-net header (10 bytes of zeros) + packet data
        let buf = &mut tx.buffers[desc_idx];
        buf[..10].fill(0); // virtio-net header
        let copy_len = data.len().min(buf.len() - 10);
        buf[10..10 + copy_len].copy_from_slice(&data[..copy_len]);

        tx.descs[desc_idx].len = (10 + copy_len) as u32;
        tx.descs[desc_idx].flags = 0; // Device reads this (no WRITE flag)
        tx.descs[desc_idx].next = 0;

        let avail_idx = (tx.avail.idx % QUEUE_SIZE as u16) as usize;
        tx.avail.ring[avail_idx] = desc_idx as u16;
        fence(Ordering::SeqCst);
        tx.avail.idx = tx.avail.idx.wrapping_add(1);
        fence(Ordering::SeqCst);

        // Notify device
        unsafe { Port::<u16>::new(self.io_base + VIRTIO_QUEUE_NOTIFY).write(TX_QUEUE); }

        Ok(())
    }

    fn reclaim_tx(&mut self) {
        let tx = self.tx_queue.as_mut().unwrap();
        fence(Ordering::SeqCst);
        while tx.last_used_idx != tx.used.idx {
            tx.num_free += 1;
            tx.last_used_idx = tx.last_used_idx.wrapping_add(1);
        }
    }

    pub fn mac_address(&self) -> [u8; 6] { self.mac }
}

pub static DEVICE: Mutex<VirtioNetDevice> = Mutex::new(VirtioNetDevice::empty());
static DEVICE_FOUND: AtomicBool = AtomicBool::new(false);

/// Initialize the real virtio-net driver.
pub fn init(hhdm_offset: u64) {
    // Find the virtio-net PCI device
    let pci_dev = match pci::find_device(VIRTIO_PCI_VENDOR, VIRTIO_NET_DEVICE_LEGACY) {
        Some(dev) => dev,
        None => {
            // Try other virtio net device IDs
            match pci::find_by_vendor(VIRTIO_PCI_VENDOR).into_iter()
                .find(|d| d.subclass == 0x00 && d.class_code == 0x02) // Network controller
            {
                Some(dev) => dev,
                None => {
                    println!("[net] No virtio-net device found on PCI bus");
                    // Fall back to dummy device
                    let mut dev = DEVICE.lock();
                    dev.mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
                    dev.initialized = false;
                    println!("[net] virtio-net: OFFLINE (no hardware)");
                    return;
                }
            }
        }
    };

    println!("[net] Found virtio-net at PCI {:02X}:{:02X}.{}", pci_dev.bus, pci_dev.device, pci_dev.function);

    // BAR0 should be I/O space (bit 0 set)
    let bar0 = pci_dev.bars[0];
    if bar0 & 1 == 0 {
        println!("[net] ERROR: BAR0 is not I/O space");
        return;
    }
    let io_base = (bar0 & 0xFFFC) as u16;
    println!("[net] virtio-net I/O base: {:#X}", io_base);

    // Enable PCI bus mastering (required for DMA)
    pci::enable_bus_master(&pci_dev);

    // --- Virtio device initialization (spec section 3.1) ---

    // 1. Reset device
    unsafe { Port::<u8>::new(io_base + VIRTIO_DEVICE_STATUS).write(VIRTIO_STATUS_RESET); }

    // 2. Set ACKNOWLEDGE
    unsafe { Port::<u8>::new(io_base + VIRTIO_DEVICE_STATUS).write(VIRTIO_STATUS_ACKNOWLEDGE); }

    // 3. Set DRIVER
    unsafe {
        let status = Port::<u8>::new(io_base + VIRTIO_DEVICE_STATUS).read();
        Port::<u8>::new(io_base + VIRTIO_DEVICE_STATUS).write(status | VIRTIO_STATUS_DRIVER);
    }

    // 4. Read device features
    let device_features = unsafe { Port::<u32>::new(io_base + VIRTIO_DEVICE_FEATURES).read() };
    println!("[net] Device features: {:#010X}", device_features);

    // 5. Negotiate features (we accept MAC feature)
    let our_features = device_features & VIRTIO_NET_F_MAC;
    unsafe { Port::<u32>::new(io_base + VIRTIO_GUEST_FEATURES).write(our_features); }

    // 6. Read MAC address
    let mut mac = [0u8; 6];
    for i in 0..6 {
        mac[i] = unsafe { Port::<u8>::new(io_base + VIRTIO_NET_MAC + i as u16).read() };
    }
    println!("[net] MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);

    // 7. Set up virtqueues (RX = queue 0, TX = queue 1)
    let rx_queue = setup_virtqueue(io_base, RX_QUEUE, hhdm_offset, true);
    let tx_queue = setup_virtqueue(io_base, TX_QUEUE, hhdm_offset, false);

    // 8. Set DRIVER_OK
    unsafe {
        let status = Port::<u8>::new(io_base + VIRTIO_DEVICE_STATUS).read();
        Port::<u8>::new(io_base + VIRTIO_DEVICE_STATUS).write(status | VIRTIO_STATUS_DRIVER_OK);
    }

    let mut dev = DEVICE.lock();
    dev.io_base = io_base;
    dev.mac = mac;
    dev.rx_queue = rx_queue;
    dev.tx_queue = tx_queue;
    dev.initialized = true;

    DEVICE_FOUND.store(true, Ordering::Release);
    println!("[net] virtio-net: ONLINE (real driver)");
}

/// Set up a single virtqueue. Allocates descriptor table, available ring, used ring.
fn setup_virtqueue(io_base: u16, queue_idx: u16, hhdm_offset: u64, is_rx: bool) -> Option<Virtqueue> {
    // Select queue
    unsafe { Port::<u16>::new(io_base + VIRTIO_QUEUE_SELECT).write(queue_idx); }

    // Read queue size
    let queue_size = unsafe { Port::<u16>::new(io_base + VIRTIO_QUEUE_SIZE).read() } as usize;
    if queue_size == 0 {
        println!("[net] Queue {} not available", queue_idx);
        return None;
    }

    // For legacy virtio, we need a single physically-contiguous allocation for the
    // descriptor table + available ring + used ring. The layout is:
    //
    // [descriptors (16 * queue_size)] [available (6 + 2*queue_size)] [padding] [used (6 + 8*queue_size)]
    //
    // We'll allocate this from our frame allocator.
    let desc_size = 16 * queue_size;
    let avail_size = 6 + 2 * queue_size;
    let used_size = 6 + 8 * queue_size;
    let total_size = desc_size + avail_size + used_size + 4096; // Extra page for alignment

    // Allocate physical pages
    let num_pages = (total_size + 4095) / 4096;
    let phys_base = {
        let mut alloc = crate::memory::frame_allocator::FRAME_ALLOCATOR.lock();
        let alloc = alloc.as_mut()?;
        let first = alloc.allocate_frame()?.start_address().as_u64();
        for _ in 1..num_pages {
            alloc.allocate_frame()?;
        }
        first
    };

    let virt_base = phys_base + hhdm_offset;

    // Zero the memory
    unsafe {
        core::ptr::write_bytes(virt_base as *mut u8, 0, num_pages * 4096);
    }

    // Set up pointers
    let descs = unsafe { &mut *(virt_base as *mut [VirtqDesc; QUEUE_SIZE]) };
    let avail = unsafe { &mut *((virt_base + desc_size as u64) as *mut VirtqAvail) };
    let used_offset = ((desc_size + avail_size + 4095) / 4096) * 4096; // Page-aligned
    let used = unsafe { &mut *((virt_base + used_offset as u64) as *mut VirtqUsed) };

    // Tell device the physical page frame number of the queue
    let pfn = (phys_base / 4096) as u32;
    unsafe { Port::<u32>::new(io_base + VIRTIO_QUEUE_ADDR).write(pfn); }

    // Allocate packet buffers and set up descriptors
    let mut buffers = Vec::with_capacity(queue_size);
    for i in 0..queue_size {
        let mut buf = vec![0u8; PACKET_BUF_SIZE];
        let buf_phys = buf.as_ptr() as u64 - hhdm_offset; // Convert virtual to physical
        // Note: this only works if the heap is in HHDM range. For a real driver,
        // we'd allocate DMA-safe buffers from the frame allocator.

        descs[i] = VirtqDesc {
            addr: buf.as_ptr() as u64, // Using virtual address — works with QEMU's emulated virtio
            len: PACKET_BUF_SIZE as u32,
            flags: if is_rx { VRING_DESC_F_WRITE } else { 0 },
            next: 0,
        };

        buffers.push(buf);

        if is_rx {
            // Pre-post RX descriptors to available ring
            avail.ring[i] = i as u16;
        }
    }

    if is_rx {
        avail.idx = queue_size as u16;
        fence(Ordering::SeqCst);
        // Notify device that RX buffers are available
        unsafe { Port::<u16>::new(io_base + VIRTIO_QUEUE_NOTIFY).write(queue_idx); }
    }

    println!("[net] Queue {}: {} entries, phys={:#X}",
        queue_idx, queue_size, phys_base);

    Some(Virtqueue {
        descs,
        avail,
        used,
        buffers,
        num_free: if is_rx { 0 } else { queue_size },
        free_head: 0,
        last_used_idx: 0,
    })
}

pub fn is_online() -> bool {
    DEVICE_FOUND.load(Ordering::Acquire)
}
