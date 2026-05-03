# Changelog

All notable changes to Oxide OS are documented here.

## [Unreleased]

### Phase 6: Networking (Next)

---

## [0.5.0] - 2026-05-03

### Phase 5: Agent Lifecycle

**Added:**
- `Agent` struct wrapping Tasks with rich metadata (config, model binding, tools, caps)
- `AgentRegistry` with BTreeMap storage and name-based lookup
- `spawn()` — creates task, registers mailbox, registers agent, adds to parent's children
- `kill()` — recursive child kill, unregister, notify parent for supervision
- `suspend()` / `resume()` for pausing agents
- Supervision trees: parent-child relationships with restart policies
- RestartOne, RestartAll, Escalate, Permanent policies
- Max restart counter with escalation (prevents infinite restart loops)
- `print_tree()` for debugging agent hierarchy
- `AgentConfig` with system_prompt, model, tools, capabilities, resource limits

**Architecture decisions:**
- AgentId = TaskId (same ID space, agent wraps task)
- Registry separate from scheduler (clean separation of concerns)
- Supervision logic in dedicated module (supervisor.rs)
- Kill cascades to children before notifying parent

---

## [0.4.0] - 2026-05-03

### Phase 4: IPC (Inter-Process Communication)

**Added:**
- Async message passing with per-task mailboxes (bounded, 256 capacity)
- `send()` / `receive()` with capability validation on every message
- Shared memory regions with physical frame allocation and cap-gated grants
- Pub/sub named channels with subscribe/publish (best-effort delivery)
- Synchronous request/reply with tick-based timeout
- Capability transfer via IPC messages (delegate on send)
- `check_timeouts()` integrated into timer interrupt (every 50 ticks)
- `register_mailbox()` / `unregister_mailbox()` for task lifecycle
- Recipient wake-on-message for blocked tasks

**Fixed:**
- `check_timeouts()` uses `try_lock` (prevents deadlock from ISR context)

**Architecture decisions:**
- Messages are kernel-buffered (copied, not zero-copy for data messages)
- Shared memory is zero-copy (both tasks map same physical frames)
- Pub/sub is best-effort (full mailbox = message dropped for that subscriber)
- All IPC operations gated by capability system
- Lock ordering: CAP_TABLE → MAILBOXES → SCHEDULER (prevents deadlock)

---

## [0.3.0] - 2026-05-03

### Phase 3: Capability System

**Added:**
- `PermissionBits` bitfield: READ, WRITE, EXECUTE, DELEGATE, SPAWN, KILL, SUBSCRIBE, PUBLISH, CONNECT
- `ResourceRef` enum: Agent, Memory, Network, Storage, Channel, Tool, Model, AgentSpawn, System
- `CapabilityTable`: global kernel capability store with O(1) lookup by ID
- `create_root()`: create system-level capabilities at boot
- `validate()`: check task ownership + permission bits on every access
- `delegate()`: create child capability with subset permissions + resource narrowing
- `revoke()`: cascading revocation (revoke parent → all children revoked)
- Per-task capability set: `has_capability()`, `grant_capability()`, `revoke_capability()`
- Kernel logging for all capability operations
- Memory region subset validation (child region must be within parent)
- Network wildcard (host=`*`, port=0 means any) for delegation
- Storage path-prefix validation for delegation

**Architecture decisions:**
- Capabilities are unforgeable kernel objects (indices into table)
- No ambient authority: zero capabilities = can do nothing
- Ownership stored in capability (table-side), mirrored in task (task-side)
- `spin::Mutex<CapabilityTable>` for thread-safe access

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
