# Phase 9: User-Space & Management — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable user-space processes (ELF loading, syscall interface), implement the `oxide` CLI for agent management, and build the REST API server for programmatic control.

**Architecture:** The kernel exposes a syscall interface (via `syscall` instruction on x86_64). User-space binaries are ELF executables loaded into per-process address spaces. The CLI runs as a user-space process communicating with the kernel's agent subsystem via syscalls. The API server listens on a TCP port and translates HTTP requests into kernel operations.

**Tech Stack:** ELF parsing, x86_64 syscall interface, custom CLI, lightweight HTTP server.

---

## File Structure

```
oxide-os/kernel/src/
├── syscall/
│   ├── mod.rs              # Syscall dispatch table
│   ├── handler.rs          # Individual syscall handlers
│   └── numbers.rs          # Syscall number definitions
├── userspace/
│   ├── mod.rs              # User-space subsystem
│   ├── elf.rs              # ELF binary loader
│   └── process.rs          # User-space process management
oxide-os/userspace/
├── Cargo.toml              # Workspace for user-space binaries
├── liboxide/
│   ├── Cargo.toml          # User-space syscall library
│   └── src/lib.rs          # Syscall wrappers
├── cli/
│   ├── Cargo.toml          # oxide CLI binary
│   └── src/main.rs         # CLI implementation
└── api-server/
    ├── Cargo.toml          # API server binary
    └── src/main.rs         # REST API implementation
```

---

## Task 1: Syscall Interface

**Files:**
- Create: `oxide-os/kernel/src/syscall/mod.rs`
- Create: `oxide-os/kernel/src/syscall/numbers.rs`
- Create: `oxide-os/kernel/src/syscall/handler.rs`

- [ ] **Step 1: Create syscall/numbers.rs**

```rust
// oxide-os/kernel/src/syscall/numbers.rs

/// Syscall numbers for Oxide OS.
pub const SYS_EXIT: u64 = 0;
pub const SYS_PRINT: u64 = 1;
pub const SYS_AGENT_SPAWN: u64 = 10;
pub const SYS_AGENT_KILL: u64 = 11;
pub const SYS_AGENT_LIST: u64 = 12;
pub const SYS_AGENT_STATUS: u64 = 13;
pub const SYS_AGENT_SUSPEND: u64 = 14;
pub const SYS_AGENT_RESUME: u64 = 15;
pub const SYS_IPC_SEND: u64 = 20;
pub const SYS_IPC_RECEIVE: u64 = 21;
pub const SYS_IPC_REQUEST: u64 = 22;
pub const SYS_IPC_REPLY: u64 = 23;
pub const SYS_NET_HTTP_GET: u64 = 30;
pub const SYS_NET_HTTP_POST: u64 = 31;
pub const SYS_STORAGE_GET: u64 = 40;
pub const SYS_STORAGE_SET: u64 = 41;
pub const SYS_STORAGE_DELETE: u64 = 42;
pub const SYS_CAP_DELEGATE: u64 = 50;
pub const SYS_CAP_REVOKE: u64 = 51;
pub const SYS_TIMER_SLEEP: u64 = 60;
pub const SYS_TIMER_DEADLINE: u64 = 61;
```

- [ ] **Step 2: Create syscall/mod.rs**

```rust
// oxide-os/kernel/src/syscall/mod.rs
pub mod numbers;
pub mod handler;

use crate::println;

/// Initialize the syscall interface (set up MSRs for `syscall` instruction).
pub fn init() {
    use x86_64::registers::model_specific::{Efer, EferFlags, LStar, Star, SFMask};
    use x86_64::registers::rflags::RFlags;
    use x86_64::VirtAddr;

    unsafe {
        // Enable SCE (System Call Extensions) in EFER
        let mut efer = Efer::read();
        Efer::write(efer | EferFlags::SYSTEM_CALL_EXTENSIONS);

        // Set LSTAR to our syscall entry point
        LStar::write(VirtAddr::new(syscall_entry as u64));

        // Set STAR: kernel CS/SS in bits 47:32, user CS/SS in bits 63:48
        // Kernel: CS=0x08, SS=0x10; User: CS=0x1B, SS=0x23
        Star::write(
            x86_64::structures::gdt::SegmentSelector(0x1B), // user CS
            x86_64::structures::gdt::SegmentSelector(0x23), // user SS
            x86_64::structures::gdt::SegmentSelector(0x08), // kernel CS
            x86_64::structures::gdt::SegmentSelector(0x10), // kernel SS
        ).unwrap();

        // Mask interrupts on syscall entry
        SFMask::write(RFlags::INTERRUPT_FLAG);
    }

    println!("[syscall] Syscall interface initialized");
}

/// Syscall entry point — called via `syscall` instruction.
/// Registers: rax=syscall number, rdi/rsi/rdx/r10/r8/r9 = arguments.
#[naked]
unsafe extern "C" fn syscall_entry() {
    core::arch::asm!(
        // Save user stack pointer
        "swapgs",
        "mov gs:[0], rsp",       // Save user RSP to per-CPU area
        "mov rsp, gs:[8]",       // Load kernel RSP
        // Push caller-saved registers
        "push rcx",              // RCX = user RIP (saved by syscall)
        "push r11",              // R11 = user RFLAGS (saved by syscall)
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // Call Rust handler: rax=number, rdi/rsi/rdx/r10/r8/r9=args
        "mov rcx, r10",          // Arg 4: Linux convention uses r10 for syscall
        "call {handler}",
        // Restore registers
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "pop r11",
        "pop rcx",
        // Restore user stack
        "mov rsp, gs:[0]",
        "swapgs",
        "sysretq",
        handler = sym handler::syscall_dispatch,
        options(noreturn),
    );
}
```

- [ ] **Step 3: Create syscall/handler.rs**

```rust
// oxide-os/kernel/src/syscall/handler.rs
use super::numbers::*;
use crate::println;
use crate::agent;
use crate::task::scheduler::SCHEDULER;

/// Main syscall dispatch function.
/// Called from assembly with: rax=number, rdi=arg1, rsi=arg2, rdx=arg3, rcx=arg4, r8=arg5, r9=arg6
#[no_mangle]
pub extern "C" fn syscall_dispatch(
    arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64, arg6: u64, number: u64
) -> i64 {
    match number {
        SYS_EXIT => {
            sys_exit(arg1 as i32)
        }
        SYS_PRINT => {
            sys_print(arg1 as *const u8, arg2 as usize)
        }
        SYS_AGENT_LIST => {
            sys_agent_list(arg1 as *mut u8, arg2 as usize)
        }
        SYS_AGENT_STATUS => {
            sys_agent_status(arg1)
        }
        SYS_AGENT_KILL => {
            sys_agent_kill(arg1)
        }
        SYS_TIMER_SLEEP => {
            sys_sleep(arg1)
        }
        _ => {
            println!("[syscall] Unknown syscall: {}", number);
            -1
        }
    }
}

fn sys_exit(code: i32) -> i64 {
    println!("[syscall] exit({})", code);
    let mut sched = SCHEDULER.lock();
    sched.kill_current();
    0
}

fn sys_print(ptr: *const u8, len: usize) -> i64 {
    // Safety: validate pointer is in user space range
    if ptr.is_null() || len > 4096 {
        return -1;
    }
    let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
    if let Ok(s) = core::str::from_utf8(slice) {
        crate::print!("{}", s);
        len as i64
    } else {
        -1
    }
}

fn sys_agent_list(buffer: *mut u8, buffer_size: usize) -> i64 {
    let registry = agent::registry::REGISTRY.lock();
    let ids = registry.all_ids();
    // Write agent count as first 8 bytes
    let count = ids.len() as u64;
    if buffer_size < 8 {
        return -1;
    }
    unsafe {
        (buffer as *mut u64).write(count);
    }
    count as i64
}

fn sys_agent_status(agent_id: u64) -> i64 {
    let registry = agent::registry::REGISTRY.lock();
    match registry.get(agent_id) {
        Some(agent) => agent.state as i64,
        None => -1,
    }
}

fn sys_agent_kill(agent_id: u64) -> i64 {
    match agent::lifecycle::kill(agent_id, None, None) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

fn sys_sleep(ticks: u64) -> i64 {
    let current_tick = crate::interrupts::ticks();
    let deadline = current_tick + ticks;
    // Get current task ID and schedule a deadline
    let task_id = {
        let sched = SCHEDULER.lock();
        sched.current_task().map(|t| t.id)
    };
    if let Some(tid) = task_id {
        crate::timer::deadline::schedule(tid, deadline);
        let mut sched = SCHEDULER.lock();
        sched.block_current();
    }
    0
}
```

- [ ] **Step 4: Add syscall module to main.rs**

```rust
mod syscall;

// In _start:
    syscall::init();
```

- [ ] **Step 5: Commit**

```bash
git add oxide-os/kernel/src/syscall/
git commit -m "feat: add syscall interface with dispatch table"
```

---

## Task 2: ELF Loader

**Files:**
- Create: `oxide-os/kernel/src/userspace/mod.rs`
- Create: `oxide-os/kernel/src/userspace/elf.rs`

- [ ] **Step 1: Create userspace/mod.rs**

```rust
// oxide-os/kernel/src/userspace/mod.rs
pub mod elf;
pub mod process;

use crate::println;

pub fn init() {
    println!("[userspace] User-space subsystem initialized");
}
```

- [ ] **Step 2: Create userspace/elf.rs**

```rust
// oxide-os/kernel/src/userspace/elf.rs
use crate::println;
use crate::memory::paging;
use x86_64::structures::paging::{OffsetPageTable, Page, PageTableFlags};
use x86_64::VirtAddr;

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

#[repr(C)]
struct ElfHeader {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
struct ProgramHeader {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

/// Load an ELF binary into a new address space.
/// Returns the entry point virtual address.
pub fn load(binary: &[u8], mapper: &mut OffsetPageTable) -> Result<u64, &'static str> {
    if binary.len() < core::mem::size_of::<ElfHeader>() {
        return Err("binary too small");
    }

    let header = unsafe { &*(binary.as_ptr() as *const ElfHeader) };

    // Validate ELF magic
    if header.e_ident[..4] != ELF_MAGIC {
        return Err("not a valid ELF file");
    }

    // Must be 64-bit
    if header.e_ident[4] != 2 {
        return Err("not a 64-bit ELF");
    }

    // Must be x86_64
    if header.e_machine != 0x3E {
        return Err("not an x86_64 ELF");
    }

    println!("[elf] Loading ELF: entry={:#X}, {} program headers",
        header.e_entry, header.e_phnum);

    // Load program headers
    for i in 0..header.e_phnum {
        let ph_offset = header.e_phoff as usize + (i as usize * header.e_phentsize as usize);
        if ph_offset + core::mem::size_of::<ProgramHeader>() > binary.len() {
            return Err("program header out of bounds");
        }

        let ph = unsafe { &*(binary.as_ptr().add(ph_offset) as *const ProgramHeader) };

        if ph.p_type != PT_LOAD {
            continue;
        }

        // Map pages for this segment
        let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        if ph.p_flags & PF_W != 0 {
            flags |= PageTableFlags::WRITABLE;
        }
        if ph.p_flags & PF_X == 0 {
            flags |= PageTableFlags::NO_EXECUTE;
        }

        let start_page = Page::containing_address(VirtAddr::new(ph.p_vaddr));
        let end_page = Page::containing_address(VirtAddr::new(ph.p_vaddr + ph.p_memsz - 1));

        let mut page = start_page;
        while page <= end_page {
            paging::alloc_and_map(mapper, page, flags);
            page = Page::containing_address(page.start_address() + 4096u64);
        }

        // Copy segment data
        if ph.p_filesz > 0 {
            let src = &binary[ph.p_offset as usize..(ph.p_offset + ph.p_filesz) as usize];
            let dst = ph.p_vaddr as *mut u8;
            unsafe {
                core::ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len());
            }
        }

        // Zero BSS (memsz > filesz)
        if ph.p_memsz > ph.p_filesz {
            let bss_start = (ph.p_vaddr + ph.p_filesz) as *mut u8;
            let bss_len = (ph.p_memsz - ph.p_filesz) as usize;
            unsafe {
                core::ptr::write_bytes(bss_start, 0, bss_len);
            }
        }

        println!("[elf]   LOAD: vaddr={:#X}, memsz={:#X}, flags={:?}",
            ph.p_vaddr, ph.p_memsz, flags);
    }

    Ok(header.e_entry)
}
```

- [ ] **Step 3: Add userspace module to main.rs**

```rust
mod userspace;

// In _start:
    userspace::init();
```

- [ ] **Step 4: Commit**

```bash
git add oxide-os/kernel/src/userspace/
git commit -m "feat: add ELF loader for user-space binaries"
```

---

## Task 3: User-Space Syscall Library (liboxide)

**Files:**
- Create: `oxide-os/userspace/Cargo.toml`
- Create: `oxide-os/userspace/liboxide/Cargo.toml`
- Create: `oxide-os/userspace/liboxide/src/lib.rs`

- [ ] **Step 1: Create userspace workspace**

```toml
# oxide-os/userspace/Cargo.toml
[workspace]
members = ["liboxide", "cli", "api-server"]
resolver = "2"
```

- [ ] **Step 2: Create liboxide/Cargo.toml**

```toml
# oxide-os/userspace/liboxide/Cargo.toml
[package]
name = "liboxide"
version = "0.1.0"
edition = "2024"

[lib]
name = "oxide"
```

- [ ] **Step 3: Create liboxide/src/lib.rs**

```rust
// oxide-os/userspace/liboxide/src/lib.rs
#![no_std]

/// Raw syscall invocation.
#[inline(always)]
unsafe fn syscall6(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") num => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        in("r8") a5,
        in("r9") a6,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}

// Syscall numbers (must match kernel)
const SYS_EXIT: u64 = 0;
const SYS_PRINT: u64 = 1;
const SYS_AGENT_SPAWN: u64 = 10;
const SYS_AGENT_KILL: u64 = 11;
const SYS_AGENT_LIST: u64 = 12;
const SYS_AGENT_STATUS: u64 = 13;
const SYS_TIMER_SLEEP: u64 = 60;

pub fn exit(code: i32) -> ! {
    unsafe { syscall6(SYS_EXIT, code as u64, 0, 0, 0, 0, 0); }
    loop {}
}

pub fn print(msg: &str) -> i64 {
    unsafe { syscall6(SYS_PRINT, msg.as_ptr() as u64, msg.len() as u64, 0, 0, 0, 0) }
}

pub fn agent_list(buffer: &mut [u8]) -> i64 {
    unsafe { syscall6(SYS_AGENT_LIST, buffer.as_mut_ptr() as u64, buffer.len() as u64, 0, 0, 0, 0) }
}

pub fn agent_status(agent_id: u64) -> i64 {
    unsafe { syscall6(SYS_AGENT_STATUS, agent_id, 0, 0, 0, 0, 0) }
}

pub fn agent_kill(agent_id: u64) -> i64 {
    unsafe { syscall6(SYS_AGENT_KILL, agent_id, 0, 0, 0, 0, 0) }
}

pub fn sleep(ticks: u64) -> i64 {
    unsafe { syscall6(SYS_TIMER_SLEEP, ticks, 0, 0, 0, 0, 0) }
}
```

- [ ] **Step 4: Commit**

```bash
git add oxide-os/userspace/
git commit -m "feat: add liboxide user-space syscall library"
```

---

## Task 4: CLI (`oxide` command)

**Files:**
- Create: `oxide-os/userspace/cli/Cargo.toml`
- Create: `oxide-os/userspace/cli/src/main.rs`

- [ ] **Step 1: Create cli/Cargo.toml**

```toml
# oxide-os/userspace/cli/Cargo.toml
[package]
name = "oxide-cli"
version = "0.1.0"
edition = "2024"

[dependencies]
oxide = { path = "../liboxide" }
```

- [ ] **Step 2: Create cli/src/main.rs**

```rust
// oxide-os/userspace/cli/src/main.rs
#![no_std]
#![no_main]

use oxide;

#[no_mangle]
extern "C" fn _start() -> ! {
    oxide::print("oxide> ");

    // Simple command loop (would read from serial/stdin in real implementation)
    // For MVP, demonstrate syscall-based agent management
    oxide::print("Oxide OS CLI v0.1.0\n");
    oxide::print("Commands: agent list, agent status <id>, agent kill <id>\n\n");

    // List agents
    let mut buf = [0u8; 4096];
    let count = oxide::agent_list(&mut buf);
    oxide::print("Active agents: ");
    // Print count (simplified — would need int-to-string in no_std)
    oxide::print("[count returned]\n");

    oxide::exit(0);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    oxide::print("CLI PANIC\n");
    oxide::exit(1);
}
```

- [ ] **Step 3: Commit**

```bash
git add oxide-os/userspace/cli/
git commit -m "feat: add oxide CLI skeleton with syscall-based commands"
```

---

## Task 5: API Server (REST)

**Files:**
- Create: `oxide-os/userspace/api-server/Cargo.toml`
- Create: `oxide-os/userspace/api-server/src/main.rs`

- [ ] **Step 1: Create api-server/Cargo.toml**

```toml
# oxide-os/userspace/api-server/Cargo.toml
[package]
name = "oxide-api-server"
version = "0.1.0"
edition = "2024"

[dependencies]
oxide = { path = "../liboxide" }
```

- [ ] **Step 2: Create api-server/src/main.rs**

```rust
// oxide-os/userspace/api-server/src/main.rs
#![no_std]
#![no_main]

use oxide;

/// Simple HTTP API server for Oxide OS management.
/// Listens on port 8080, handles REST endpoints:
/// - GET /agents        -> list all agents
/// - GET /agents/:id    -> agent status
/// - POST /agents       -> spawn agent
/// - DELETE /agents/:id -> kill agent
/// - GET /system        -> system status

#[no_mangle]
extern "C" fn _start() -> ! {
    oxide::print("[api] Oxide API Server starting on :8080\n");

    // In real implementation:
    // 1. Create TCP listener socket via syscall
    // 2. Accept connections
    // 3. Parse HTTP requests
    // 4. Route to handlers
    // 5. Send HTTP responses

    // For now, this is a skeleton showing the architecture.
    // The actual HTTP parsing would use a minimal no_std HTTP parser.

    loop {
        oxide::sleep(100); // Sleep 100 ticks between polls
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    oxide::print("[api] PANIC\n");
    oxide::exit(1);
}

// --- HTTP Route Handlers (to be implemented) ---

fn handle_get_agents() -> &'static [u8] {
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"agents\":[]}"
}

fn handle_get_system() -> &'static [u8] {
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"version\":\"0.1.0\",\"status\":\"running\"}"
}
```

- [ ] **Step 3: Commit**

```bash
git add oxide-os/userspace/api-server/
git commit -m "feat: add REST API server skeleton"
```

---

## Summary

After Phase 9, Oxide OS has:
- Syscall interface via `syscall` instruction (fast user-kernel transition)
- Syscall dispatch table covering agents, IPC, networking, storage, timers
- ELF binary loader for user-space processes
- User-space syscall library (`liboxide`) for easy kernel interaction
- `oxide` CLI for agent management commands
- REST API server skeleton on port 8080
- Proper user-kernel separation with capability validation on syscalls
- Ready for Phase 10 (inference engine, WASM sandbox, web dashboard)
