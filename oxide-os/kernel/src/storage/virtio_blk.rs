//! Real virtio-blk driver using legacy (transitional) PCI transport.
//!
//! Implements the virtio 1.0 legacy interface for block devices.
//! Uses a single split virtqueue for synchronous block I/O (read/write).
//!
//! References:
//! - https://docs.oasis-open.org/virtio/virtio/v1.1/virtio-v1.1.html (section 5.2)
//! - QEMU virtio-blk-pci device

use spin::Mutex;
use core::sync::atomic::{AtomicBool, Ordering, fence};
use x86_64::instructions::port::Port;
use crate::pci;
use crate::println;

// Virtio PCI vendor ID
const VIRTIO_PCI_VENDOR: u16 = 0x1AF4;
// Block device: device ID 0x1001 (legacy)
const VIRTIO_BLK_DEVICE_LEGACY: u16 = 0x1001;

// Legacy virtio PCI register offsets (from BAR0, I/O space)
const VIRTIO_DEVICE_FEATURES: u16 = 0x00;   // 4 bytes, read
const VIRTIO_GUEST_FEATURES: u16 = 0x04;    // 4 bytes, write
const VIRTIO_QUEUE_ADDR: u16 = 0x08;        // 4 bytes, write (PFN)
const VIRTIO_QUEUE_SIZE: u16 = 0x0C;        // 2 bytes, read
const VIRTIO_QUEUE_SELECT: u16 = 0x0E;      // 2 bytes, write
const VIRTIO_QUEUE_NOTIFY: u16 = 0x10;      // 2 bytes, write
const VIRTIO_DEVICE_STATUS: u16 = 0x12;     // 1 byte, read/write
const VIRTIO_ISR_STATUS: u16 = 0x13;        // 1 byte, read
// Device-specific config starts at 0x14
const VIRTIO_BLK_CAPACITY: u16 = 0x14;      // 8 bytes (capacity in 512-byte sectors)

// Virtio status bits
const VIRTIO_STATUS_RESET: u8 = 0;
const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
const VIRTIO_STATUS_DRIVER: u8 = 2;
const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
const _VIRTIO_STATUS_FEATURES_OK: u8 = 8;

// Virtqueue descriptor flags
const VRING_DESC_F_NEXT: u16 = 1;
const VRING_DESC_F_WRITE: u16 = 2;

// Block request types (spec 5.2.6)
const VIRTIO_BLK_T_IN: u32 = 0;   // Read
const VIRTIO_BLK_T_OUT: u32 = 1;  // Write

// Block request status values
const VIRTIO_BLK_S_OK: u8 = 0;
const _VIRTIO_BLK_S_IOERR: u8 = 1;
const _VIRTIO_BLK_S_UNSUPP: u8 = 2;

const QUEUE_SIZE: usize = 128;
const REQUEST_QUEUE: u16 = 0;

/// Virtio block request header (spec 5.2.6)
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioBlkReqHeader {
    type_: u32,
    _reserved: u32,
    sector: u64,
}

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
    /// Next available descriptor index for building chains
    next_desc: u16,
    /// Last seen used index
    last_used_idx: u16,
}

/// The virtio-blk device state.
pub struct VirtioBlkDevice {
    io_base: u16,
    capacity_blocks: u64,
    queue: Option<Virtqueue>,
    /// Physical address of a DMA buffer region for request headers/data/status
    dma_phys: u64,
    /// Virtual address of the DMA buffer region
    dma_virt: u64,
    initialized: bool,
}

impl VirtioBlkDevice {
    pub const fn empty() -> Self {
        VirtioBlkDevice {
            io_base: 0,
            capacity_blocks: 0,
            queue: None,
            dma_phys: 0,
            dma_virt: 0,
            initialized: false,
        }
    }

    /// Read a 512-byte block from disk.
    pub fn read_block(&mut self, block_num: u64, buffer: &mut [u8; 512]) -> Result<(), &'static str> {
        if !self.initialized { return Err("not initialized"); }
        if block_num >= self.capacity_blocks { return Err("block out of range"); }
        self.do_block_io(VIRTIO_BLK_T_IN, block_num, buffer)
    }

    /// Write a 512-byte block to disk.
    pub fn write_block(&mut self, block_num: u64, data: &[u8; 512]) -> Result<(), &'static str> {
        if !self.initialized { return Err("not initialized"); }
        if block_num >= self.capacity_blocks { return Err("block out of range"); }
        // Copy data into a mutable buffer for the unified I/O path
        let mut buf = *data;
        self.do_block_io(VIRTIO_BLK_T_OUT, block_num, &mut buf)
    }

    /// Perform a block I/O operation using the virtqueue.
    ///
    /// Layout in DMA buffer:
    ///   Offset 0:    VirtioBlkReqHeader (16 bytes)
    ///   Offset 16:   Data buffer (512 bytes)
    ///   Offset 528:  Status byte (1 byte)
    fn do_block_io(&mut self, req_type: u32, sector: u64, buffer: &mut [u8; 512]) -> Result<(), &'static str> {
        let queue = self.queue.as_mut().ok_or("no virtqueue")?;

        let header_phys = self.dma_phys;
        let data_phys = self.dma_phys + 16;
        let status_phys = self.dma_phys + 16 + 512;

        let header_virt = self.dma_virt;
        let data_virt = self.dma_virt + 16;
        let status_virt = self.dma_virt + 16 + 512;

        // Write the request header
        unsafe {
            let header = &mut *(header_virt as *mut VirtioBlkReqHeader);
            header.type_ = req_type;
            header._reserved = 0;
            header.sector = sector;
        }

        // For writes, copy data into the DMA buffer
        if req_type == VIRTIO_BLK_T_OUT {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    buffer.as_ptr(),
                    data_virt as *mut u8,
                    512,
                );
            }
        }

        // Write status byte to 0xFF (so we can detect completion)
        unsafe { *(status_virt as *mut u8) = 0xFF; }

        // Use 3 chained descriptors
        let desc_base = queue.next_desc;
        let d0 = desc_base as usize % QUEUE_SIZE;
        let d1 = (desc_base as usize + 1) % QUEUE_SIZE;
        let d2 = (desc_base as usize + 2) % QUEUE_SIZE;

        // Descriptor 0: request header (device reads)
        queue.descs[d0] = VirtqDesc {
            addr: header_phys,
            len: 16, // sizeof VirtioBlkReqHeader
            flags: VRING_DESC_F_NEXT,
            next: d1 as u16,
        };

        // Descriptor 1: data buffer
        queue.descs[d1] = VirtqDesc {
            addr: data_phys,
            len: 512,
            flags: VRING_DESC_F_NEXT | if req_type == VIRTIO_BLK_T_IN { VRING_DESC_F_WRITE } else { 0 },
            next: d2 as u16,
        };

        // Descriptor 2: status byte (device writes)
        queue.descs[d2] = VirtqDesc {
            addr: status_phys,
            len: 1,
            flags: VRING_DESC_F_WRITE,
            next: 0,
        };

        // Advance next_desc for future requests
        queue.next_desc = (desc_base + 3) % QUEUE_SIZE as u16;

        // Add head of chain to available ring
        let avail_idx = queue.avail.idx;
        let avail_slot = (avail_idx % QUEUE_SIZE as u16) as usize;
        queue.avail.ring[avail_slot] = d0 as u16;

        fence(Ordering::SeqCst);
        queue.avail.idx = avail_idx.wrapping_add(1);
        fence(Ordering::SeqCst);

        // Notify device
        unsafe { Port::<u16>::new(self.io_base + VIRTIO_QUEUE_NOTIFY).write(REQUEST_QUEUE); }

        // Spin-wait for completion (synchronous I/O)
        let mut spin_count: u64 = 0;
        loop {
            fence(Ordering::SeqCst);
            if queue.used.idx != queue.last_used_idx {
                break;
            }
            spin_count += 1;
            if spin_count > 100_000_000 {
                return Err("virtio-blk: timeout waiting for I/O");
            }
            core::hint::spin_loop();
        }

        queue.last_used_idx = queue.last_used_idx.wrapping_add(1);

        // Check status byte
        let status = unsafe { *(status_virt as *const u8) };
        if status != VIRTIO_BLK_S_OK {
            return Err("virtio-blk: I/O error");
        }

        // For reads, copy data out of DMA buffer
        if req_type == VIRTIO_BLK_T_IN {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    data_virt as *const u8,
                    buffer.as_mut_ptr(),
                    512,
                );
            }
        }

        Ok(())
    }

    pub fn capacity(&self) -> u64 { self.capacity_blocks }
}

pub static DEVICE: Mutex<VirtioBlkDevice> = Mutex::new(VirtioBlkDevice::empty());
static DEVICE_FOUND: AtomicBool = AtomicBool::new(false);

/// Initialize the real virtio-blk driver.
pub fn init(hhdm_offset: u64) {
    // Find the virtio-blk PCI device
    let pci_dev = match pci::find_device(VIRTIO_PCI_VENDOR, VIRTIO_BLK_DEVICE_LEGACY) {
        Some(dev) => dev,
        None => {
            // Try finding any virtio device with mass storage class (0x01)
            match pci::find_by_vendor(VIRTIO_PCI_VENDOR).into_iter()
                .find(|d| d.class_code == 0x01) // Mass storage controller
            {
                Some(dev) => dev,
                None => {
                    println!("[storage] WARNING: No virtio-blk device found on PCI bus");
                    println!("[storage] virtio-blk: OFFLINE (no hardware)");
                    // Graceful degradation: leave device uninitialized
                    return;
                }
            }
        }
    };

    println!("[storage] Found virtio-blk at PCI {:02X}:{:02X}.{}",
        pci_dev.bus, pci_dev.device, pci_dev.function);

    // BAR0 should be I/O space (bit 0 set)
    let bar0 = pci_dev.bars[0];
    if bar0 & 1 == 0 {
        println!("[storage] ERROR: BAR0 is not I/O space");
        return;
    }
    let io_base = (bar0 & 0xFFFC) as u16;
    println!("[storage] virtio-blk I/O base: {:#X}", io_base);

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
    println!("[storage] Device features: {:#010X}", device_features);

    // 5. Negotiate features (accept none for basic operation)
    unsafe { Port::<u32>::new(io_base + VIRTIO_GUEST_FEATURES).write(0); }

    // 6. Read disk capacity from device-specific config at offset 0x14 (8 bytes)
    let cap_lo = unsafe { Port::<u32>::new(io_base + VIRTIO_BLK_CAPACITY).read() } as u64;
    let cap_hi = unsafe { Port::<u32>::new(io_base + VIRTIO_BLK_CAPACITY + 4).read() } as u64;
    let capacity = cap_lo | (cap_hi << 32);
    println!("[storage] Disk capacity: {} sectors ({} MiB)", capacity, capacity * 512 / 1024 / 1024);

    // 7. Set up the request virtqueue (queue 0)
    let (queue, dma_phys, dma_virt) = match setup_virtqueue(io_base, REQUEST_QUEUE, hhdm_offset) {
        Some(result) => result,
        None => {
            println!("[storage] ERROR: Failed to set up virtqueue");
            return;
        }
    };

    // 8. Set DRIVER_OK
    unsafe {
        let status = Port::<u8>::new(io_base + VIRTIO_DEVICE_STATUS).read();
        Port::<u8>::new(io_base + VIRTIO_DEVICE_STATUS).write(status | VIRTIO_STATUS_DRIVER_OK);
    }

    let mut dev = DEVICE.lock();
    dev.io_base = io_base;
    dev.capacity_blocks = capacity;
    dev.queue = Some(queue);
    dev.dma_phys = dma_phys;
    dev.dma_virt = dma_virt;
    dev.initialized = true;

    DEVICE_FOUND.store(true, Ordering::Release);
    println!("[storage] virtio-blk: ONLINE (real driver, {} blocks)", capacity);
}

/// Set up the virtqueue and allocate a DMA buffer for block requests.
/// Returns (Virtqueue, dma_phys, dma_virt).
fn setup_virtqueue(io_base: u16, queue_idx: u16, hhdm_offset: u64) -> Option<(Virtqueue, u64, u64)> {
    // Select queue
    unsafe { Port::<u16>::new(io_base + VIRTIO_QUEUE_SELECT).write(queue_idx); }

    // Read queue size
    let queue_size = unsafe { Port::<u16>::new(io_base + VIRTIO_QUEUE_SIZE).read() } as usize;
    if queue_size == 0 {
        println!("[storage] Queue {} not available", queue_idx);
        return None;
    }
    println!("[storage] Queue {}: device reports {} entries", queue_idx, queue_size);

    // For legacy virtio, layout is:
    // [descriptors (16 * queue_size)] [available (6 + 2*queue_size)] [padding to page] [used (6 + 8*queue_size)]
    let desc_size = 16 * QUEUE_SIZE;
    let avail_size = 6 + 2 * QUEUE_SIZE;
    let used_size = 6 + 8 * QUEUE_SIZE;
    let total_size = desc_size + avail_size + used_size + 4096; // Extra page for alignment

    // Allocate physical pages for virtqueue structures
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

    // Set up pointers to virtqueue structures
    let descs = unsafe { &mut *(virt_base as *mut [VirtqDesc; QUEUE_SIZE]) };
    let avail = unsafe { &mut *((virt_base + desc_size as u64) as *mut VirtqAvail) };
    let used_offset = ((desc_size + avail_size + 4095) / 4096) * 4096; // Page-aligned
    let used = unsafe { &mut *((virt_base + used_offset as u64) as *mut VirtqUsed) };

    // Tell device the physical page frame number of the queue
    let pfn = (phys_base / 4096) as u32;
    unsafe { Port::<u32>::new(io_base + VIRTIO_QUEUE_ADDR).write(pfn); }

    // Allocate a DMA buffer for block I/O requests (1 page is plenty)
    // Layout: [header: 16 bytes] [data: 512 bytes] [status: 1 byte]
    let dma_phys = {
        let mut alloc = crate::memory::frame_allocator::FRAME_ALLOCATOR.lock();
        let alloc = alloc.as_mut()?;
        alloc.allocate_frame()?.start_address().as_u64()
    };
    let dma_virt = dma_phys + hhdm_offset;

    // Zero the DMA buffer
    unsafe {
        core::ptr::write_bytes(dma_virt as *mut u8, 0, 4096);
    }

    println!("[storage] Queue {}: {} entries, phys={:#X}, dma={:#X}",
        queue_idx, QUEUE_SIZE, phys_base, dma_phys);

    Some((
        Virtqueue {
            descs,
            avail,
            used,
            next_desc: 0,
            last_used_idx: 0,
        },
        dma_phys,
        dma_virt,
    ))
}

pub fn is_online() -> bool {
    DEVICE_FOUND.load(Ordering::Acquire)
}
