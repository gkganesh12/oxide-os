# Changelog

All notable changes to Oxide OS are documented here.

## [Unreleased]

### Phase 3: Capability System (Next)

---

## [0.2.0] - 2026-05-03

### Phase 2: Preemptive Scheduler

**Added:**
- Local APIC driver with periodic timer interrupt (~400 Hz)
- Multi-level priority scheduler (Realtime > Normal > Background)
- Preemptive multitasking via deferred context switching
- Per-task kernel stacks (16 KiB virtual, contiguous, with guard page)
- Naked-function `context_switch` assembly (callee-saved registers)
- Fair scheduling algorithm (round-robin across all priorities)
- Task entry trampoline (enables interrupts on fresh tasks)
- `yield_now()` for cooperative scheduling points
- `block_and_yield()` for tasks waiting on resources
- `exit_current()` for clean task termination
- `should_reschedule()` flag for timer-driven preemption

**Fixed (from initial implementation):**
- Non-contiguous stack allocation → mapped virtual pages contiguously
- Interrupt frame leak per context switch → deferred-switch model
- Interrupts disabled forever on fresh tasks → entry trampoline with `sti`
- `static mut` in APIC → `AtomicU64`
- Task monopolization → fair `pick_next_fair` skips last-run task
- Scheduler lock held during context switch → drop before switch

**Architecture decisions:**
- Timer ISR sets flag only; actual switch at function-call level
- Context stored in fixed array (pointers stable across queue moves)
- Guard page per task stack (page fault on overflow)

---

## [0.1.0] - 2026-05-03

### Phase 1: Kernel Boot & Foundation

**Added:**
- Boots in QEMU via Limine bootloader (BIOS + UEFI)
- UART 16550 serial console output
- Deadlock-free panic printing (bypasses serial lock)
- GDT with TSS (page-aligned double-fault stack)
- IDT with exception handlers (breakpoint, double fault, page fault, GPF, invalid opcode)
- Bitmap physical frame allocator (tracks usable memory only)
- Kernel + bootloader memory protection in frame allocator
- Virtual memory management (4-level page tables via HHDM)
- 1 MiB kernel heap (linked-list allocator at 0xFFFFA00000000000)
- `#[alloc_error_handler]` for clear OOM messages
- QEMU debug exit device support
- GitHub Actions CI (build + boot test)

**Architecture decisions:**
- Limine boot protocol (HHDM, structured memory map)
- Kernel at 0xFFFFFFFF80000000 (higher-half)
- Heap at 0xFFFFA00000000000 (avoids HHDM conflict)
- `spin` crate for kernel mutexes (no_std compatible)
- `x86_64` crate for CPU structures (battle-tested)
