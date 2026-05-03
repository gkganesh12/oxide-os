# Changelog

All notable changes to Oxide OS are documented here.

## [Unreleased]

---

## [1.2.0] - 2026-05-03

### Production Hardening

**Added:**
- OxideFS disk persistence: `sync_to_disk()` / `load_from_disk()` with superblock + blob + index serialization
- Ring-3 user-mode foundation: `enter_usermode()` via iretq, user code/data GDT segments
- TLS 1.3 stub: interface for HTTPS, availability check, https:// URL detection in HTTP client
- User-mode GDT entries (user code at ring 3, user data at ring 3)
- OxideFS auto-loads from disk on init (graceful fallback to fresh if no FS found)

**Architecture decisions:**
- OxideFS disk format: block 0 superblock (magic+counts), blocks 1..N blobs (id+size+data), blocks N+1..M index entries
- Ring-3 uses iretq for privilege transition (standard x86_64 mechanism)
- TLS is a stub — real implementation requires rustls crate integration

## [1.1.0] - 2026-05-03

### Real I/O Stack — Making Everything Actually Work

**Added:**
- PCI bus enumeration (scans all bus/device/function, discovers hardware)
- Real virtio-net driver (PCI discovery, BAR0, virtqueue RX/TX, feature negotiation, DMA)
- Real virtio-blk driver (PCI discovery, virtqueue block I/O, 3-descriptor chains)
- Real HTTP over TCP (socket create → connect → send → receive → parse, with timeouts)
- Syscall instruction wiring (EFER.SCE, LSTAR, STAR, SFMASK, naked assembly entry)
- HTTP response parser (status line + body extraction)
- PCI bus mastering for DMA transfers

**Fixed:**
- Agent signing `verify()` no longer returns `true` unconditionally — real HMAC comparison
- HTTP validates capabilities AND calls firewall before connecting
- smoltcp Device trait wired to real virtio-net virtqueue RX/TX

**What was placeholder → now real:**
- `virtio-net`: skeleton → real PCI driver with 256-entry split virtqueues
- `virtio-blk`: stub returning zeros → real PCI driver with virtqueue block I/O
- `HTTP client`: returning `{}` → real TCP socket flow with timeouts
- `signing.verify()`: always true → constant-time HMAC comparison
- `syscall dispatch`: Rust functions only → x86_64 MSRs wired for `syscall` instruction

## [1.0.0] - 2026-05-03

### Phase 10: Inference Engine & GPU Scheduler

**Added:**
- GPU/NPU inference scheduler with priority queue (Urgent > Normal > Batch)
- Deadline-based request expiration
- Capability-gated inference submission (EXECUTE permission required)
- Request tracking with completion/failure counters
- "All 10 subsystems operational" boot banner

### Full Release — All 10 Phases Complete

Oxide OS v1.0.0 is the first complete implementation of the agent-native microkernel:

| Subsystem | Phase | Lines |
|-----------|-------|-------|
| Boot (Limine, GDT, IDT) | 1 | 400 |
| Preemptive Scheduler | 2 | 500 |
| Capability Security | 3 | 380 |
| IPC (msg, shm, pub/sub, req/reply) | 4 | 470 |
| Agent Lifecycle | 5 | 340 |
| Networking (TCP/IP, HTTP, firewall) | 6 | 370 |
| Storage (OxideFS, context store) | 7 | 315 |
| Crypto (RNG, HMAC, signing) | 8 | 440 |
| Syscalls & ELF Loader | 9 | 265 |
| GPU Inference Scheduler | 10 | 160 |
| **Total** | | **~4,400** |

---

## [0.9.0] - 2026-05-03

### Phase 9: User-Space & Management

**Added:**
- Syscall dispatch table with 20 defined syscall numbers
- 11 implemented handlers: exit, print, yield, agent_list, agent_status, agent_kill, ipc_send, ipc_receive, storage_set, storage_get, sleep
- Input validation on all syscall handlers (null checks, length limits)
- ELF64 loader: header parsing, magic validation, program header enumeration
- Process struct for future user-space isolation
- All handlers integrated with existing kernel subsystems (agents, IPC, storage, timer)

**Architecture decisions:**
- Syscall handlers are kernel-callable functions (will be wired to `syscall` instruction in hardening phase)
- Print syscall caps output at 4 KiB (prevent kernel log flooding)
- IPC send caps payload at 64 KiB
- Storage key capped at 256 bytes, value at 64 KiB

---

## [0.8.0] - 2026-05-03

### Phase 8: Crypto & Timers

**Added:**
- Hardware RNG: RDRAND detection via CPUID, XorShift64 fallback seeded from TSC
- `fill_bytes()` for filling buffers with random data
- HMAC-SHA256 for capability token generation and validation
- Constant-time tag comparison (prevents timing attacks)
- Boot-generated HMAC key (unique per boot, never leaves kernel)
- Per-agent keypair generation and HMAC-based signing
- System clock with TSC-based monotonic time (millis, seconds)
- Deadline queue (min-heap) for time-sensitive scheduling
- Deadline expiration integrated into timer interrupt handler

**Architecture decisions:**
- sha2 crate with `force-soft` (no asm dependency for cross-compilation)
- HMAC key generated from RNG at boot (not hardcoded)
- Deadline queue uses `try_lock` from ISR (deadlock-safe)
- Signing uses HMAC-SHA256 (Ed25519 deferred to production hardening)

---

## [0.7.0] - 2026-05-03

### Phase 7: Storage

**Added:**
- virtio-blk block device driver skeleton (512 MiB virtual disk)
- LRU block cache (256 entries, 128 KiB, write-back eviction)
- OxideFS: log-structured filesystem with content-addressable blob dedup
- Per-agent context store (key-value, capability-gated read/write)
- `clear_agent()` for cleanup on agent death
- File operations: write, read, delete, list by prefix

**Architecture decisions:**
- In-memory filesystem for MVP (backed by blobs, not disk blocks yet)
- Content dedup via full-content comparison (hash-based dedup deferred)
- Context store has in-memory cache + OxideFS persistence
- Cap validation: CAP_TABLE → STORE → FS lock ordering

---

## [0.6.0] - 2026-05-03

### Phase 6: Networking

**Added:**
- virtio-net driver skeleton (MAC, receive/transmit interface)
- smoltcp v0.12 TCP/IP stack integration (Ethernet, IPv4, TCP, UDP)
- Network interface: 10.0.2.15/24 with gateway 10.0.2.2 (QEMU user-mode)
- Socket API: tcp_create, tcp_connect (cap-gated), tcp_send, tcp_receive, tcp_close
- Cache-based DNS resolver with static entries
- HTTP client (GET/POST) with URL parsing
- Capability-gated firewall (checks host/port against Network capabilities)
- Network stack polling function for timer-driven I/O

**Architecture decisions:**
- smoltcp Device trait wraps virtio-net (swappable backend)
- HTTP returns placeholder responses until real virtio packet handling is implemented
- Firewall validates at connect time, not per-packet (performance)
- Ephemeral port allocation via atomic counter

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
