# Phase 1: Kernel Boot & Foundation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Boot Oxide OS in QEMU, establish serial output, physical/virtual memory management, and a kernel heap — the foundation for all subsequent phases.

**Architecture:** x86_64 freestanding Rust binary using the Limine boot protocol. Limine handles BIOS/UEFI boot and passes a rich boot info structure (memory map, HHDM offset). The kernel initializes GDT, IDT (basic exceptions), a bitmap frame allocator, 4-level page tables, and a linked-list heap allocator.

**Tech Stack:** Rust nightly (`#![no_std]`), Limine bootloader, `limine` crate (boot protocol), `x86_64` crate (CPU structures), `spin` crate (spinlock mutexes), QEMU x86_64 for testing.

---

## File Structure

```
oxide-os/
├── Cargo.toml                    # Workspace root
├── kernel/
│   ├── Cargo.toml                # Kernel crate
│   ├── src/
│   │   ├── main.rs               # Entry point, kernel_main
│   │   ├── serial.rs             # UART 16550 serial output
│   │   ├── gdt.rs                # Global Descriptor Table
│   │   ├── interrupts.rs         # IDT + exception handlers
│   │   ├── memory/
│   │   │   ├── mod.rs            # Memory subsystem root
│   │   │   ├── frame_allocator.rs # Physical frame allocator (bitmap)
│   │   │   └── paging.rs         # Page table management
│   │   └── allocator.rs          # Kernel heap allocator
│   ├── linker.ld                 # Linker script for kernel
│   └── .cargo/
│       └── config.toml           # Build target + linker flags
├── limine.conf                   # Limine bootloader config
├── Makefile                      # Build + run commands
└── tests/
    └── boot_test.rs              # QEMU integration test
```

---

## Task 1: Project Setup & Custom Target

**Files:**
- Create: `oxide-os/Cargo.toml`
- Create: `oxide-os/kernel/Cargo.toml`
- Create: `oxide-os/kernel/src/main.rs`
- Create: `oxide-os/kernel/.cargo/config.toml`
- Create: `oxide-os/kernel/linker.ld`
- Create: `oxide-os/rust-toolchain.toml`

- [ ] **Step 1: Create workspace Cargo.toml**

```toml
# oxide-os/Cargo.toml
[workspace]
members = ["kernel"]
resolver = "2"
```

- [ ] **Step 2: Create kernel Cargo.toml**

```toml
# oxide-os/kernel/Cargo.toml
[package]
name = "oxide-kernel"
version = "0.1.0"
edition = "2024"

[dependencies]
limine = "0.4"
spin = "0.9"
x86_64 = "0.15"
uart_16550 = "0.3"
linked_list_allocator = "0.10"

[profile.dev]
panic = "abort"

[profile.release]
panic = "abort"
lto = true
```

- [ ] **Step 3: Create rust-toolchain.toml**

```toml
# oxide-os/rust-toolchain.toml
[toolchain]
channel = "nightly"
components = ["rust-src", "llvm-tools-preview"]
targets = ["x86_64-unknown-none"]
```

- [ ] **Step 4: Create .cargo/config.toml**

```toml
# oxide-os/kernel/.cargo/config.toml
[build]
target = "x86_64-unknown-none"

[target.x86_64-unknown-none]
rustflags = ["-C", "link-arg=-Tlinker.ld", "-C", "link-arg=-nostdlib"]
```

- [ ] **Step 5: Create linker script**

```ld
/* oxide-os/kernel/linker.ld */
ENTRY(_start)

PHDRS {
    text    PT_LOAD FLAGS(5);  /* r-x */
    rodata  PT_LOAD FLAGS(4);  /* r-- */
    data    PT_LOAD FLAGS(6);  /* rw- */
}

SECTIONS {
    . = 0xFFFFFFFF80000000;

    .text : {
        *(.text .text.*)
    } :text

    . = ALIGN(4096);
    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    . = ALIGN(4096);
    .data : {
        *(.data .data.*)
    } :data

    .bss : {
        *(COMMON)
        *(.bss .bss.*)
    } :data

    /DISCARD/ : {
        *(.eh_frame)
        *(.note .note.*)
    }
}
```

- [ ] **Step 6: Create minimal main.rs (halt loop)**

```rust
// oxide-os/kernel/src/main.rs
#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
extern "C" fn _start() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
```

- [ ] **Step 7: Verify it compiles**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles successfully, produces ELF binary at `target/x86_64-unknown-none/debug/oxide-kernel`

- [ ] **Step 8: Commit**

```bash
git add oxide-os/
git commit -m "feat: initial project setup with custom x86_64 target"
```

---

## Task 2: Limine Bootloader Integration

**Files:**
- Modify: `oxide-os/kernel/src/main.rs`
- Create: `oxide-os/limine.conf`
- Create: `oxide-os/Makefile`

- [ ] **Step 1: Update main.rs to use Limine boot protocol**

```rust
// oxide-os/kernel/src/main.rs
#![no_std]
#![no_main]

use core::panic::PanicInfo;
use limine::BaseRevision;
use limine::request::{HhdmRequest, MemoryMapRequest, StackSizeRequest};

/// Set the base revision to the latest supported by the crate.
#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

/// Request a higher-half direct map so we can access physical memory.
#[used]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

/// Request the memory map from the bootloader.
#[used]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

/// Request a 64 KiB stack.
#[used]
static STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest::new().with_size(64 * 1024);

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
extern "C" fn _start() -> ! {
    assert!(BASE_REVISION.is_supported());

    // We'll add serial output next, then memory init
    loop {
        core::hint::spin_loop();
    }
}
```

- [ ] **Step 2: Create limine.conf**

```
# oxide-os/limine.conf
timeout: 0

/Oxide OS
    protocol: limine
    kernel_path: boot():/kernel
```

- [ ] **Step 3: Create Makefile for building bootable ISO**

```makefile
# oxide-os/Makefile
KERNEL := kernel/target/x86_64-unknown-none/debug/oxide-kernel
ISO := oxide-os.iso
LIMINE_DIR := limine

.PHONY: all kernel run clean setup

all: $(ISO)

setup:
	@if [ ! -d "$(LIMINE_DIR)" ]; then \
		git clone https://github.com/limine-bootloader/limine.git --branch=v8.x-binary --depth=1; \
		$(MAKE) -C $(LIMINE_DIR); \
	fi

kernel:
	cd kernel && cargo build

$(ISO): kernel setup
	mkdir -p iso_root/boot
	cp $(KERNEL) iso_root/boot/kernel
	cp limine.conf iso_root/boot/limine.conf
	cp $(LIMINE_DIR)/limine-bios.sys iso_root/boot/
	cp $(LIMINE_DIR)/limine-bios-cd.bin iso_root/boot/
	cp $(LIMINE_DIR)/limine-uefi-cd.bin iso_root/boot/
	xorriso -as mkisofs -b boot/limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot boot/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		iso_root -o $(ISO)
	./$(LIMINE_DIR)/limine bios-install $(ISO)

run: $(ISO)
	qemu-system-x86_64 -cdrom $(ISO) -serial stdio -no-reboot -no-shutdown

clean:
	cd kernel && cargo clean
	rm -rf iso_root $(ISO)
```

- [ ] **Step 4: Build and boot in QEMU**

Run: `cd oxide-os && make run`
Expected: QEMU boots, kernel hangs (no output yet — just verifies boot works without crash). Kill with Ctrl+C.

- [ ] **Step 5: Commit**

```bash
git add oxide-os/limine.conf oxide-os/Makefile oxide-os/kernel/src/main.rs
git commit -m "feat: integrate Limine bootloader, boot in QEMU"
```

---

## Task 3: Serial Output (UART 16550)

**Files:**
- Create: `oxide-os/kernel/src/serial.rs`
- Modify: `oxide-os/kernel/src/main.rs`

- [ ] **Step 1: Create serial.rs**

```rust
// oxide-os/kernel/src/serial.rs
use spin::Mutex;
use uart_16550::SerialPort;
use core::fmt;
use core::fmt::Write;

static SERIAL: Mutex<Option<SerialPort>> = Mutex::new(None);

pub fn init() {
    let mut port = unsafe { SerialPort::new(0x3F8) };
    port.init();
    *SERIAL.lock() = Some(port);
}

pub fn _print(args: fmt::Arguments) {
    if let Some(ref mut port) = *SERIAL.lock() {
        port.write_fmt(args).unwrap();
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::serial::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
```

- [ ] **Step 2: Update main.rs to use serial output**

```rust
// oxide-os/kernel/src/main.rs
#![no_std]
#![no_main]

mod serial;

use core::panic::PanicInfo;
use limine::BaseRevision;
use limine::request::{HhdmRequest, MemoryMapRequest, StackSizeRequest};

#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
static STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest::new().with_size(64 * 1024);

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("KERNEL PANIC: {}", info);
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
extern "C" fn _start() -> ! {
    serial::init();
    assert!(BASE_REVISION.is_supported());

    println!("=== Oxide OS v0.1.0 ===");
    println!("[boot] Limine base revision supported");
    println!("[boot] Serial output initialized");

    // Print memory map info
    if let Some(response) = MEMORY_MAP_REQUEST.get_response() {
        let entries = response.entries();
        println!("[boot] Memory map: {} entries", entries.len());
        let total_usable: u64 = entries
            .iter()
            .filter(|e| e.entry_type == limine::memory_map::EntryType::USABLE)
            .map(|e| e.length)
            .sum();
        println!("[boot] Total usable memory: {} MiB", total_usable / 1024 / 1024);
    }

    println!("[boot] Kernel halted.");
    loop {
        core::hint::spin_loop();
    }
}
```

- [ ] **Step 3: Build and run — verify serial output**

Run: `cd oxide-os && make run`
Expected output on terminal:
```
=== Oxide OS v0.1.0 ===
[boot] Limine base revision supported
[boot] Serial output initialized
[boot] Memory map: <N> entries
[boot] Total usable memory: 128 MiB
[boot] Kernel halted.
```

- [ ] **Step 4: Commit**

```bash
git add oxide-os/kernel/src/serial.rs oxide-os/kernel/src/main.rs
git commit -m "feat: add UART serial output, print boot info"
```

---

## Task 4: GDT (Global Descriptor Table)

**Files:**
- Create: `oxide-os/kernel/src/gdt.rs`
- Modify: `oxide-os/kernel/src/main.rs`

- [ ] **Step 1: Create gdt.rs**

```rust
// oxide-os/kernel/src/gdt.rs
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;
use spin::Lazy;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
const STACK_SIZE: usize = 4096 * 5;

static mut DOUBLE_FAULT_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

static TSS: Lazy<TaskStateSegment> = Lazy::new(|| {
    let mut tss = TaskStateSegment::new();
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
        let stack_start = VirtAddr::from_ptr(unsafe { &DOUBLE_FAULT_STACK });
        stack_start + STACK_SIZE as u64
    };
    tss
});

static GDT: Lazy<(GlobalDescriptorTable, Selectors)> = Lazy::new(|| {
    let mut gdt = GlobalDescriptorTable::new();
    let code_selector = gdt.append(Descriptor::kernel_code_segment());
    let data_selector = gdt.append(Descriptor::kernel_data_segment());
    let tss_selector = gdt.append(Descriptor::tss_segment(&TSS));
    (gdt, Selectors { code_selector, data_selector, tss_selector })
});

struct Selectors {
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

pub fn init() {
    use x86_64::instructions::segmentation::{CS, DS, SS, Segment};
    use x86_64::instructions::tables::load_tss;

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.code_selector);
        DS::set_reg(GDT.1.data_selector);
        SS::set_reg(SegmentSelector(0));
        load_tss(GDT.1.tss_selector);
    }
}
```

- [ ] **Step 2: Add gdt module to main.rs and call init**

Add after `serial::init()` in `_start`:

```rust
mod gdt;

// In _start(), after serial::init():
    gdt::init();
    println!("[boot] GDT initialized");
```

- [ ] **Step 3: Build and run — verify GDT loads**

Run: `cd oxide-os && make run`
Expected: `[boot] GDT initialized` appears in serial output, no triple fault.

- [ ] **Step 4: Commit**

```bash
git add oxide-os/kernel/src/gdt.rs oxide-os/kernel/src/main.rs
git commit -m "feat: add GDT with TSS for double fault stack"
```

---

## Task 5: IDT (Interrupt Descriptor Table)

**Files:**
- Create: `oxide-os/kernel/src/interrupts.rs`
- Modify: `oxide-os/kernel/src/main.rs`

- [ ] **Step 1: Create interrupts.rs**

```rust
// oxide-os/kernel/src/interrupts.rs
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use spin::Lazy;
use crate::gdt;
use crate::println;

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();

    idt.breakpoint.set_handler_fn(breakpoint_handler);
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }
    idt.page_fault.set_handler_fn(page_fault_handler);
    idt.general_protection_fault.set_handler_fn(general_protection_handler);
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);

    idt
});

pub fn init() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("[interrupt] BREAKPOINT at {:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    println!("[FATAL] DOUBLE FAULT\n{:#?}", stack_frame);
    loop {
        core::hint::spin_loop();
    }
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    println!("[interrupt] PAGE FAULT");
    println!("  Accessed Address: {:?}", Cr2::read());
    println!("  Error Code: {:?}", error_code);
    println!("  {:#?}", stack_frame);
    loop {
        core::hint::spin_loop();
    }
}

extern "x86-interrupt" fn general_protection_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    println!("[FATAL] GENERAL PROTECTION FAULT (error: {})\n{:#?}", error_code, stack_frame);
    loop {
        core::hint::spin_loop();
    }
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    println!("[FATAL] INVALID OPCODE\n{:#?}", stack_frame);
    loop {
        core::hint::spin_loop();
    }
}
```

- [ ] **Step 2: Add interrupts module to main.rs and call init**

Add to main.rs:

```rust
mod interrupts;

// In _start(), after gdt::init():
    interrupts::init();
    println!("[boot] IDT initialized");
```

- [ ] **Step 3: Test with a breakpoint exception**

Add a test breakpoint in `_start` after IDT init:

```rust
    // Test: trigger breakpoint exception
    x86_64::instructions::interrupts::int3();
    println!("[boot] Survived breakpoint exception");
```

- [ ] **Step 4: Build and run — verify exception handling**

Run: `cd oxide-os && make run`
Expected output includes:
```
[boot] IDT initialized
[interrupt] BREAKPOINT at InterruptStackFrame { ... }
[boot] Survived breakpoint exception
```

- [ ] **Step 5: Remove the test breakpoint, commit**

Remove the `int3()` and "Survived" println from `_start`.

```bash
git add oxide-os/kernel/src/interrupts.rs oxide-os/kernel/src/main.rs
git commit -m "feat: add IDT with exception handlers (double fault, page fault, GPF)"
```

---

## Task 6: Physical Memory Allocator (Bitmap Frame Allocator)

**Files:**
- Create: `oxide-os/kernel/src/memory/mod.rs`
- Create: `oxide-os/kernel/src/memory/frame_allocator.rs`
- Modify: `oxide-os/kernel/src/main.rs`

- [ ] **Step 1: Create memory/mod.rs**

```rust
// oxide-os/kernel/src/memory/mod.rs
pub mod frame_allocator;
pub mod paging;

use x86_64::PhysAddr;

pub const PAGE_SIZE: u64 = 4096;

/// Convert a physical address to a virtual address using the HHDM offset.
pub fn phys_to_virt(phys: PhysAddr, hhdm_offset: u64) -> *mut u8 {
    (phys.as_u64() + hhdm_offset) as *mut u8
}
```

- [ ] **Step 2: Create frame_allocator.rs**

```rust
// oxide-os/kernel/src/memory/frame_allocator.rs
use x86_64::structures::paging::PhysFrame;
use x86_64::PhysAddr;
use spin::Mutex;
use crate::memory::PAGE_SIZE;
use crate::println;

/// Bitmap-based physical frame allocator.
/// Each bit represents one 4 KiB frame: 0 = free, 1 = allocated.
pub struct BitmapFrameAllocator {
    bitmap: &'static mut [u8],
    total_frames: usize,
    next_free: usize,
}

impl BitmapFrameAllocator {
    /// Initialize from the Limine memory map.
    /// `bitmap_storage` must point to a usable memory region large enough
    /// to hold `total_frames / 8` bytes.
    pub unsafe fn new(bitmap_storage: *mut u8, total_frames: usize) -> Self {
        let bitmap_size = (total_frames + 7) / 8;
        let bitmap = core::slice::from_raw_parts_mut(bitmap_storage, bitmap_size);
        // Mark all frames as allocated initially
        bitmap.fill(0xFF);

        BitmapFrameAllocator {
            bitmap,
            total_frames,
            next_free: 0,
        }
    }

    /// Mark a range of frames as free (usable).
    pub fn mark_region_free(&mut self, start_frame: usize, count: usize) {
        for i in start_frame..start_frame + count {
            if i < self.total_frames {
                self.clear_bit(i);
            }
        }
    }

    /// Mark a range of frames as used.
    pub fn mark_region_used(&mut self, start_frame: usize, count: usize) {
        for i in start_frame..start_frame + count {
            if i < self.total_frames {
                self.set_bit(i);
            }
        }
    }

    /// Allocate a single physical frame.
    pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
        for i in self.next_free..self.total_frames {
            if !self.is_set(i) {
                self.set_bit(i);
                self.next_free = i + 1;
                let addr = PhysAddr::new((i as u64) * PAGE_SIZE);
                return Some(PhysFrame::containing_address(addr));
            }
        }
        // Wrap around search
        for i in 0..self.next_free {
            if !self.is_set(i) {
                self.set_bit(i);
                self.next_free = i + 1;
                let addr = PhysAddr::new((i as u64) * PAGE_SIZE);
                return Some(PhysFrame::containing_address(addr));
            }
        }
        None
    }

    /// Free a physical frame.
    pub fn free_frame(&mut self, frame: PhysFrame) {
        let index = frame.start_address().as_u64() as usize / PAGE_SIZE as usize;
        self.clear_bit(index);
        if index < self.next_free {
            self.next_free = index;
        }
    }

    /// Get allocation statistics.
    pub fn stats(&self) -> (usize, usize) {
        let used = self.bitmap.iter().map(|b| b.count_ones() as usize).sum::<usize>();
        (self.total_frames - used, self.total_frames)
    }

    fn is_set(&self, index: usize) -> bool {
        let byte = index / 8;
        let bit = index % 8;
        (self.bitmap[byte] >> bit) & 1 == 1
    }

    fn set_bit(&mut self, index: usize) {
        let byte = index / 8;
        let bit = index % 8;
        self.bitmap[byte] |= 1 << bit;
    }

    fn clear_bit(&mut self, index: usize) {
        let byte = index / 8;
        let bit = index % 8;
        self.bitmap[byte] &= !(1 << bit);
    }
}

pub static FRAME_ALLOCATOR: Mutex<Option<BitmapFrameAllocator>> = Mutex::new(None);

/// Initialize the frame allocator from the Limine memory map.
pub fn init(memory_map: &[&limine::memory_map::Entry], hhdm_offset: u64) {
    // Find the highest physical address to determine total frames needed
    let max_addr = memory_map
        .iter()
        .map(|entry| entry.base + entry.length)
        .max()
        .unwrap_or(0);

    let total_frames = (max_addr / PAGE_SIZE) as usize;
    let bitmap_size = (total_frames + 7) / 8;

    println!("[memory] Total physical frames: {} ({} MiB)", total_frames, total_frames * 4096 / 1024 / 1024);
    println!("[memory] Bitmap size: {} KiB", bitmap_size / 1024);

    // Find a usable region large enough to store the bitmap
    let bitmap_region = memory_map
        .iter()
        .filter(|e| e.entry_type == limine::memory_map::EntryType::USABLE)
        .find(|e| e.length as usize >= bitmap_size)
        .expect("No usable region large enough for frame allocator bitmap");

    let bitmap_phys = bitmap_region.base;
    let bitmap_virt = (bitmap_phys + hhdm_offset) as *mut u8;

    let mut allocator = unsafe { BitmapFrameAllocator::new(bitmap_virt, total_frames) };

    // Mark usable regions as free
    for entry in memory_map.iter() {
        if entry.entry_type == limine::memory_map::EntryType::USABLE {
            let start_frame = (entry.base / PAGE_SIZE) as usize;
            let count = (entry.length / PAGE_SIZE) as usize;
            allocator.mark_region_free(start_frame, count);
        }
    }

    // Mark the bitmap itself as used
    let bitmap_start_frame = (bitmap_phys / PAGE_SIZE) as usize;
    let bitmap_frame_count = (bitmap_size as u64 + PAGE_SIZE - 1) / PAGE_SIZE;
    allocator.mark_region_used(bitmap_start_frame, bitmap_frame_count as usize);

    let (free, total) = allocator.stats();
    println!("[memory] Frame allocator initialized: {}/{} frames free ({} MiB)", free, total, free * 4096 / 1024 / 1024);

    *FRAME_ALLOCATOR.lock() = Some(allocator);
}
```

- [ ] **Step 3: Update main.rs to initialize frame allocator**

```rust
mod memory;

// In _start(), after IDT init:
    let hhdm_offset = HHDM_REQUEST
        .get_response()
        .expect("HHDM response missing")
        .offset();

    if let Some(response) = MEMORY_MAP_REQUEST.get_response() {
        let entries = response.entries();
        println!("[boot] Memory map: {} entries", entries.len());
        memory::frame_allocator::init(entries, hhdm_offset);
    } else {
        panic!("Memory map not available");
    }
```

- [ ] **Step 4: Test allocation — allocate and free a frame**

Add after frame allocator init:

```rust
    // Test frame allocation
    {
        let mut alloc = memory::frame_allocator::FRAME_ALLOCATOR.lock();
        let alloc = alloc.as_mut().unwrap();
        let frame = alloc.allocate_frame().expect("allocation failed");
        println!("[test] Allocated frame at: {:?}", frame.start_address());
        alloc.free_frame(frame);
        println!("[test] Frame freed successfully");
    }
```

- [ ] **Step 5: Build and run**

Run: `cd oxide-os && make run`
Expected:
```
[memory] Total physical frames: <N>
[memory] Frame allocator initialized: <N>/<M> frames free (<X> MiB)
[test] Allocated frame at: PhysAddr(0x...)
[test] Frame freed successfully
```

- [ ] **Step 6: Remove test code, commit**

Remove the test allocation block from `_start`.

```bash
git add oxide-os/kernel/src/memory/
git commit -m "feat: add bitmap physical frame allocator"
```

---

## Task 7: Virtual Memory (Page Table Management)

**Files:**
- Create: `oxide-os/kernel/src/memory/paging.rs`
- Modify: `oxide-os/kernel/src/memory/mod.rs`
- Modify: `oxide-os/kernel/src/main.rs`

- [ ] **Step 1: Create paging.rs**

```rust
// oxide-os/kernel/src/memory/paging.rs
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags,
    PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};
use crate::memory::frame_allocator::FRAME_ALLOCATOR;
use crate::println;

/// A frame allocator that wraps our bitmap allocator for use with x86_64 crate's Mapper.
pub struct OxideFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for OxideFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        FRAME_ALLOCATOR
            .lock()
            .as_mut()
            .and_then(|alloc| alloc.allocate_frame())
    }
}

/// Initialize the OffsetPageTable using Limine's HHDM.
/// Safety: caller must ensure hhdm_offset is correct and the level 4 table is valid.
pub unsafe fn init(hhdm_offset: u64) -> OffsetPageTable<'static> {
    let level_4_table = active_level_4_table(hhdm_offset);
    OffsetPageTable::new(level_4_table, VirtAddr::new(hhdm_offset))
}

/// Get a mutable reference to the active level 4 page table.
unsafe fn active_level_4_table(hhdm_offset: u64) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (frame, _flags) = Cr3::read();
    let phys = frame.start_address();
    let virt = VirtAddr::new(phys.as_u64() + hhdm_offset);
    let table: *mut PageTable = virt.as_mut_ptr();
    &mut *table
}

/// Map a virtual page to a physical frame.
pub fn map_page(
    mapper: &mut OffsetPageTable,
    page: Page<Size4KiB>,
    frame: PhysFrame<Size4KiB>,
    flags: PageTableFlags,
) {
    let mut frame_allocator = OxideFrameAllocator;
    unsafe {
        mapper
            .map_to(page, frame, flags, &mut frame_allocator)
            .expect("map_to failed")
            .flush();
    }
}

/// Allocate a new frame and map a virtual page to it.
pub fn alloc_and_map(
    mapper: &mut OffsetPageTable,
    page: Page<Size4KiB>,
    flags: PageTableFlags,
) -> PhysFrame<Size4KiB> {
    let frame = FRAME_ALLOCATOR
        .lock()
        .as_mut()
        .expect("frame allocator not initialized")
        .allocate_frame()
        .expect("out of physical memory");

    map_page(mapper, page, frame, flags);
    frame
}

/// Print page table stats for debugging.
pub fn print_stats(mapper: &OffsetPageTable) {
    println!("[paging] Page table initialized (4-level, offset-mapped via HHDM)");
}
```

- [ ] **Step 2: Update mod.rs to export paging**

Already done in Step 1 of Task 6 — `pub mod paging;` is in `mod.rs`.

- [ ] **Step 3: Initialize paging in main.rs**

Add after frame allocator init:

```rust
    let mut mapper = unsafe { memory::paging::init(hhdm_offset) };
    memory::paging::print_stats(&mapper);
```

- [ ] **Step 4: Test mapping a page**

Add a test that maps a page and writes to it:

```rust
    // Test: map a new page and write to it
    {
        use x86_64::structures::paging::{Page, PageTableFlags};

        let test_addr = VirtAddr::new(0xDEAD_0000);
        let test_page = Page::containing_address(test_addr);
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

        memory::paging::alloc_and_map(&mut mapper, test_page, flags);

        let ptr: *mut u64 = test_addr.as_mut_ptr();
        unsafe { *ptr = 0x_CAFE_BABE };
        let val = unsafe { *ptr };
        assert_eq!(val, 0x_CAFE_BABE);
        println!("[test] Page mapping works: wrote and read 0x{:X}", val);
    }
```

- [ ] **Step 5: Build and run**

Run: `cd oxide-os && make run`
Expected: `[test] Page mapping works: wrote and read 0xCAFEBABE`

- [ ] **Step 6: Remove test, commit**

Remove the test block.

```bash
git add oxide-os/kernel/src/memory/paging.rs oxide-os/kernel/src/main.rs
git commit -m "feat: add virtual memory page table management"
```

---

## Task 8: Kernel Heap Allocator

**Files:**
- Create: `oxide-os/kernel/src/allocator.rs`
- Modify: `oxide-os/kernel/src/main.rs`

- [ ] **Step 1: Create allocator.rs**

```rust
// oxide-os/kernel/src/allocator.rs
use linked_list_allocator::LockedHeap;
use x86_64::structures::paging::{Page, PageTableFlags, OffsetPageTable, Size4KiB};
use x86_64::VirtAddr;
use crate::memory::paging;
use crate::println;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Kernel heap starts at this virtual address.
pub const HEAP_START: u64 = 0xFFFF_8000_0000_0000;
/// Initial heap size: 1 MiB.
pub const HEAP_SIZE: u64 = 1024 * 1024;

/// Initialize the kernel heap by mapping pages and initializing the allocator.
pub fn init(mapper: &mut OffsetPageTable) {
    let heap_start = VirtAddr::new(HEAP_START);
    let heap_end = heap_start + HEAP_SIZE;

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

    let start_page = Page::<Size4KiB>::containing_address(heap_start);
    let end_page = Page::<Size4KiB>::containing_address(heap_end - 1u64);

    let mut page_count = 0u64;
    let mut current = start_page;
    while current <= end_page {
        paging::alloc_and_map(mapper, current, flags);
        current = Page::containing_address(current.start_address() + 4096u64);
        page_count += 1;
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE as usize);
    }

    println!(
        "[heap] Kernel heap initialized: {} KiB ({} pages) at {:#X}",
        HEAP_SIZE / 1024,
        page_count,
        HEAP_START
    );
}
```

- [ ] **Step 2: Update main.rs — add alloc crate and init heap**

Add to top of main.rs:

```rust
#![no_std]
#![no_main]

extern crate alloc;

mod allocator;
mod gdt;
mod interrupts;
mod memory;
mod serial;

use alloc::string::String;
use alloc::vec::Vec;
use core::panic::PanicInfo;
use limine::BaseRevision;
use limine::request::{HhdmRequest, MemoryMapRequest, StackSizeRequest};
use x86_64::VirtAddr;

#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
static STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest::new().with_size(64 * 1024);

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("KERNEL PANIC: {}", info);
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
extern "C" fn _start() -> ! {
    serial::init();
    println!("=== Oxide OS v0.1.0 ===");

    assert!(BASE_REVISION.is_supported());
    println!("[boot] Limine base revision supported");

    gdt::init();
    println!("[boot] GDT initialized");

    interrupts::init();
    println!("[boot] IDT initialized");

    let hhdm_offset = HHDM_REQUEST
        .get_response()
        .expect("HHDM response missing")
        .offset();

    if let Some(response) = MEMORY_MAP_REQUEST.get_response() {
        let entries = response.entries();
        println!("[boot] Memory map: {} entries", entries.len());
        memory::frame_allocator::init(entries, hhdm_offset);
    } else {
        panic!("Memory map not available");
    }

    let mut mapper = unsafe { memory::paging::init(hhdm_offset) };
    println!("[boot] Page tables initialized");

    allocator::init(&mut mapper);

    // Test heap allocation
    let mut v = Vec::new();
    v.push(1u64);
    v.push(2u64);
    v.push(3u64);
    println!("[test] Vec on heap: {:?}", v);

    let s = String::from("Oxide OS kernel heap works!");
    println!("[test] {}", s);

    println!("[boot] All systems initialized. Kernel ready.");
    loop {
        core::hint::spin_loop();
    }
}
```

- [ ] **Step 3: Build and run**

Run: `cd oxide-os && make run`
Expected:
```
=== Oxide OS v0.1.0 ===
[boot] Limine base revision supported
[boot] GDT initialized
[boot] IDT initialized
[boot] Memory map: <N> entries
[memory] Frame allocator initialized: ...
[boot] Page tables initialized
[heap] Kernel heap initialized: 1024 KiB (256 pages) at 0xFFFF800000000000
[test] Vec on heap: [1, 2, 3]
[test] Oxide OS kernel heap works!
[boot] All systems initialized. Kernel ready.
```

- [ ] **Step 4: Remove test code, commit**

Remove the Vec and String test lines from `_start`.

```bash
git add oxide-os/kernel/src/allocator.rs oxide-os/kernel/src/main.rs
git commit -m "feat: add kernel heap allocator (1 MiB linked-list)"
```

---

## Task 9: QEMU Exit & Test Runner

**Files:**
- Create: `oxide-os/kernel/src/qemu.rs`
- Modify: `oxide-os/kernel/src/main.rs`
- Modify: `oxide-os/Makefile`

- [ ] **Step 1: Create qemu.rs for exit device**

```rust
// oxide-os/kernel/src/qemu.rs
use x86_64::instructions::port::Port;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ExitCode {
    Success = 0x10,
    Failure = 0x11,
}

pub fn exit(code: ExitCode) {
    unsafe {
        let mut port = Port::new(0xf4);
        port.write(code as u32);
    }
}
```

- [ ] **Step 2: Add test run target to Makefile**

Add to Makefile:

```makefile
test: $(ISO)
	qemu-system-x86_64 -cdrom $(ISO) -serial stdio -no-reboot -no-shutdown \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04 \
		-display none
```

- [ ] **Step 3: Add qemu module to main.rs**

```rust
mod qemu;
```

- [ ] **Step 4: Commit**

```bash
git add oxide-os/kernel/src/qemu.rs oxide-os/Makefile oxide-os/kernel/src/main.rs
git commit -m "feat: add QEMU exit device for automated testing"
```

---

## Task 10: Final Integration & Boot Banner

**Files:**
- Modify: `oxide-os/kernel/src/main.rs`

- [ ] **Step 1: Clean up main.rs — final boot sequence**

```rust
// oxide-os/kernel/src/main.rs
#![no_std]
#![no_main]

extern crate alloc;

mod allocator;
mod gdt;
mod interrupts;
mod memory;
mod qemu;
mod serial;

use core::panic::PanicInfo;
use limine::BaseRevision;
use limine::request::{HhdmRequest, MemoryMapRequest, StackSizeRequest};

#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
static STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest::new().with_size(64 * 1024);

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("KERNEL PANIC: {}", info);
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
extern "C" fn _start() -> ! {
    serial::init();

    println!();
    println!("  ╔══════════════════════════════════╗");
    println!("  ║        Oxide OS v0.1.0           ║");
    println!("  ║   Agent-Native Microkernel       ║");
    println!("  ╚══════════════════════════════════╝");
    println!();

    assert!(BASE_REVISION.is_supported());
    println!("[boot] Limine protocol OK");

    gdt::init();
    println!("[boot] GDT loaded");

    interrupts::init();
    println!("[boot] IDT loaded");

    let hhdm_offset = HHDM_REQUEST
        .get_response()
        .expect("HHDM response missing")
        .offset();

    if let Some(response) = MEMORY_MAP_REQUEST.get_response() {
        memory::frame_allocator::init(response.entries(), hhdm_offset);
    } else {
        panic!("Memory map not available");
    }

    let mut mapper = unsafe { memory::paging::init(hhdm_offset) };
    println!("[boot] Page tables ready");

    allocator::init(&mut mapper);

    println!();
    println!("[boot] Phase 1 complete — kernel foundation operational");
    println!("[boot] Awaiting Phase 2: Scheduler & Interrupts");
    println!();

    // Halt the CPU in a low-power loop
    loop {
        x86_64::instructions::hlt();
    }
}
```

- [ ] **Step 2: Build and verify final boot**

Run: `cd oxide-os && make run`
Expected:
```
  ╔══════════════════════════════════╗
  ║        Oxide OS v0.1.0           ║
  ║   Agent-Native Microkernel       ║
  ╚══════════════════════════════════╝

[boot] Limine protocol OK
[boot] GDT loaded
[boot] IDT loaded
[memory] Total physical frames: <N> (<M> MiB)
[memory] Frame allocator initialized: <X>/<Y> frames free (<Z> MiB)
[boot] Page tables ready
[heap] Kernel heap initialized: 1024 KiB (256 pages) at 0xFFFF800000000000

[boot] Phase 1 complete — kernel foundation operational
[boot] Awaiting Phase 2: Scheduler & Interrupts
```

- [ ] **Step 3: Commit**

```bash
git add oxide-os/kernel/src/main.rs
git commit -m "feat: finalize Phase 1 boot sequence with banner"
```

---

## Summary

After completing Phase 1, Oxide OS will:
- Boot in QEMU via Limine bootloader
- Output to serial console
- Have a proper GDT with TSS (double fault stack)
- Handle CPU exceptions (page faults, double faults, GPF)
- Manage physical memory via bitmap frame allocator
- Support virtual memory mapping (4-level page tables)
- Have a 1 MiB kernel heap with `alloc` support (Vec, String, Box, etc.)
- Be ready for Phase 2 (scheduler, timer interrupts, preemptive multitasking)

## Next Phases (separate plans)

- **Phase 2:** Interrupts & Preemptive Scheduler
- **Phase 3:** Capability System
- **Phase 4:** IPC (Message Passing, Shared Memory)
- **Phase 5:** Agent Lifecycle Management
- **Phase 6:** Networking (virtio-net, TCP/IP, HTTP)
- **Phase 7:** Storage (virtio-blk, OxideFS)
- **Phase 8:** Crypto & Timers
- **Phase 9:** User-space, CLI, API Server
- **Phase 10:** Inference Engine & Web Dashboard
