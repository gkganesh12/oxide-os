# Oxide OS

**An agent-native microkernel operating system written from scratch in Rust.**

Built for running AI agent swarms with kernel-level isolation, capability-based security, and real hardware drivers. Built in Rust. Boots on bare metal via QEMU.

---

## Why?

AI agents are everywhere. But they all run on general-purpose operating systems designed for humans.

| Problem | How Linux handles it | How Oxide OS handles it |
|---------|---------------------|------------------------|
| Agent isolation | Docker containers (heavyweight) | Kernel-level capability tokens (zero overhead) |
| Agent communication | Redis/RabbitMQ (external service) | Built-in IPC: messages, shared memory, pub/sub |
| Agent crashes | Manual restart, no coordination | Supervision trees with auto-restart policies |
| Agent permissions | Coarse (root or not root) | Per-agent, per-resource, revocable capabilities |
| Network access control | iptables rules (misconfigurable) | Kernel-enforced capability firewall |

**Oxide OS treats AI agents as first-class kernel primitives.** Each agent gets its own capabilities, IPC channels, storage, and supervision — enforced by the kernel, not by convention.

---

## Live Boot Output

```
  ╔══════════════════════════════════════╗
  ║        Oxide OS v1.1.0               ║
  ║   Agent-Native Microkernel (Rust)    ║
  ╚══════════════════════════════════════╝

[boot] Limine protocol OK
[boot] GDT loaded | IDT loaded
[memory] Frame allocator: 30097/32582 frames free (117 MiB)
[pci] Found 7 devices
[pci]   00:03.0 vendor=1AF4 device=1000 class=02:00    <- virtio-net
[pci]   00:04.0 vendor=1AF4 device=1001 class=01:00    <- virtio-blk
[net] Found virtio-net at PCI 00:03.0
[net] virtio-net I/O base: 0xC080
[net] MAC: 52:54:00:12:34:56
[net] Queue 0: 256 entries (RX)
[net] Queue 1: 256 entries (TX)
[net] virtio-net: ONLINE (real driver)
[net] TCP/IP stack: 10.0.2.15/24, gateway 10.0.2.2
[storage] Found virtio-blk at PCI 00:04.0
[storage] virtio-blk: ONLINE (real driver)
[storage] OxideFS initialized (log-structured, content-addressable)
[crypto] RNG + HMAC-SHA256 + agent signing ready
[gpu] Inference scheduler initialized
[syscall] MSRs configured (EFER.SCE, LSTAR, STAR, SFMASK)
[syscall] 20 calls registered

  ╔══════════════════════════════════════════╗
  ║    All subsystems operational            ║
  ╚══════════════════════════════════════════╝

[cap] Created root #1 for task 1 -> AgentSpawn [D|SPAWN|KILL]
[cap] Created root #2 for task 2 -> Agent(4) [W]
[cap] Created root #3 for task 3 -> Agent(4) [W]

[demo] Agent supervision tree:
|- supervisor [id:1, Running, restarts:0]
  |- researcher-1 [id:2, Running, restarts:0]
  |- researcher-2 [id:3, Running, restarts:0]
  |- aggregator [id:4, Running, restarts:0]

[researcher-1] Sent finding #1 to aggregator
[aggregator] Received finding #1: {"agent":"researcher-1","finding":1,"topic":"AI alignment"}
[researcher-2] Sent finding #1 to aggregator
[aggregator] Received finding #2: {"agent":"researcher-2","finding":1,"topic":"gradient optimization"}
[supervisor] Status #1: 4 agents, 4 tasks, tick=509
[aggregator] *** Summary: 5 total findings collected ***
```

---

## What's Real

This is not a wrapper or a toy. The kernel talks to real hardware:

### PCI Bus Enumeration
Scans the PCI configuration space, discovers devices by vendor/device ID, reads BARs, enables bus mastering for DMA.

### Real Virtio-Net Driver
Legacy PCI transport with split virtqueues (256-entry RX + TX). Feature negotiation, MAC address from hardware, DMA ring buffers. Packets flow through smoltcp's TCP/IP stack.

### Real Virtio-Blk Driver
PCI discovery, virtqueue setup, 3-descriptor block request chains (header + data + status). Synchronous read/write with spin-wait on used ring.

### Real Syscall Wiring
x86_64 MSRs configured: EFER.SCE enabled, LSTAR points to naked assembly entry, STAR sets kernel/user segment selectors, SFMASK masks interrupts. The `syscall` instruction is hardware-ready.

### Real Context Switching
8 lines of x86_64 assembly save/restore callee-saved registers. APIC timer fires at ~400 Hz. Deferred-switch model prevents interrupt frame leaks.

---

## Architecture

```
              ┌─────────────────────────────────────────┐
              │            Agent Swarm                   │
              │  ┌───────────┐  ┌───────────────────┐   │
              │  │ Supervisor│──│ restart policies   │   │
              │  └─────┬─────┘  └───────────────────┘   │
              │    ┌───┴───┬───────────┐                │
              │    │       │           │                │
              │ ┌──┴──┐ ┌──┴──┐ ┌─────┴─────┐         │
              │ │Agent│ │Agent│ │ Aggregator │         │
              │ │  1  │ │  2  │ │  (Agent 3) │         │
              │ └──┬──┘ └──┬──┘ └─────┬─────┘         │
              │    │       │          │                │
              │    └───────┴────┬─────┘                │
              │           IPC Messages                  │
              ├─────────────────────────────────────────┤
              │                                         │
              │         Kernel (5,148 lines Rust)       │
              │                                         │
              │  ┌──────────┐ ┌──────────┐ ┌────────┐  │
              │  │Scheduler │ │Capability│ │  IPC   │  │
              │  │ 3-level  │ │  System  │ │msg/shm │  │
              │  │ priority │ │ delegate │ │pub/sub │  │
              │  │ preempt  │ │ revoke   │ │req/rep │  │
              │  └──────────┘ └──────────┘ └────────┘  │
              │  ┌──────────┐ ┌──────────┐ ┌────────┐  │
              │  │Networking│ │ Storage  │ │ Crypto │  │
              │  │ smoltcp  │ │ OxideFS  │ │HMAC256 │  │
              │  │ HTTP/TCP │ │ blk cache│ │  RNG   │  │
              │  │ firewall │ │ KV store │ │signing │  │
              │  └──────────┘ └──────────┘ └────────┘  │
              │  ┌──────────┐ ┌──────────┐ ┌────────┐  │
              │  │ Syscalls │ │GPU Sched │ │ Timer  │  │
              │  │ 20 calls │ │ priority │ │deadline│  │
              │  │ x86 MSRs │ │ deadlines│ │ queue  │  │
              │  └──────────┘ └──────────┘ └────────┘  │
              ├─────────────────────────────────────────┤
              │              Hardware                    │
              │  PCI Bus │ virtio-net │ virtio-blk      │
              │  APIC    │ UART 16550 │ x86_64 CPU      │
              └─────────────────────────────────────────┘
```

### Kernel Source Structure

```
oxide-os/kernel/src/               5,148 lines total
├── main.rs                        Boot sequence + agent swarm demo
├── pci/mod.rs                     PCI bus enumeration (real hardware discovery)
├── net/
│   ├── virtio_net.rs              Real virtio-net driver (PCI + virtqueues)
│   ├── stack.rs                   smoltcp TCP/IP integration
│   ├── socket.rs                  Capability-gated TCP socket API
│   ├── http.rs                    Real HTTP client (TCP connect + parse)
│   ├── dns.rs                     Caching DNS resolver
│   └── firewall.rs                Capability-gated network access control
├── storage/
│   ├── virtio_blk.rs              Real virtio-blk driver (PCI + virtqueue I/O)
│   ├── block_cache.rs             LRU block cache with write-back
│   ├── oxide_fs.rs                Log-structured FS with content-dedup (FNV-1a)
│   └── context_store.rs           Per-agent key-value store (cap-gated)
├── capability/
│   ├── permissions.rs             Permission bitfield (R/W/X/D/SPAWN/KILL/...)
│   ├── resource.rs                Resource types (Agent, Network, Storage, ...)
│   └── table.rs                   Global capability table (create/delegate/revoke)
├── task/
│   ├── mod.rs                     Task struct, stack alloc, guard pages, trampoline
│   ├── context.rs                 Naked context_switch assembly
│   └── scheduler.rs               Multi-priority fair scheduler, yield, block
├── agent/
│   ├── lifecycle.rs               Spawn, kill, suspend, resume
│   ├── registry.rs                Agent registry with tree printing
│   └── supervisor.rs              Restart policies (RestartOne/All/Escalate)
├── ipc/
│   ├── message.rs                 Async mailboxes (256 capacity, cap-gated)
│   ├── shared_memory.rs           Zero-copy shared regions
│   ├── channel.rs                 Pub/sub named channels
│   └── request_reply.rs           Synchronous req/reply with timeout
├── crypto/
│   ├── rng.rs                     RDRAND + XorShift64 fallback
│   ├── hmac_cap.rs                HMAC-SHA256 capability token validation
│   └── signing.rs                 Per-agent keypair + real verification
├── syscall/
│   ├── numbers.rs                 20 syscall definitions
│   ├── handler.rs                 Dispatch table + 11 handlers
│   └── mod.rs                     x86_64 MSR setup + naked entry point
├── gpu/scheduler.rs               Inference request queue (priority + deadlines)
├── timer/
│   ├── clock.rs                   TSC-based monotonic system clock
│   └── deadline.rs                Min-heap deadline queue
├── serial.rs                      UART 16550 (deadlock-free panic output)
├── gdt.rs                         GDT + TSS (page-aligned double-fault stack)
├── interrupts.rs                  IDT + exception handlers + APIC timer
├── apic.rs                        Local APIC (MMIO mapped, atomic base)
├── allocator.rs                   1 MiB kernel heap (linked-list)
├── memory/
│   ├── frame_allocator.rs         Bitmap physical frame allocator
│   └── paging.rs                  4-level page tables via HHDM
└── qemu.rs                       Debug exit device
```

---

## Key Design Decisions

### Capability-Based Security (not UNIX permissions)

Every resource access requires a capability token — an unforgeable kernel object:

```
Agent 2 holds: cap #2 → Agent(4) [WRITE]
  Can send messages to Agent 4 ✓
  Cannot spawn new agents    ✗
  Cannot access the network  ✗
  Cannot read storage        ✗
```

Capabilities can be **delegated** (with equal or fewer permissions) and **revoked** (cascading to all children). An agent with zero capabilities can do nothing.

### Supervision Trees (not "hope it doesn't crash")

```
supervisor (RestartOne policy)
├── researcher-1   → crashes → auto-restart, siblings unaffected
├── researcher-2   → crashes → auto-restart, siblings unaffected
└── aggregator     → crashes → auto-restart

If restart count exceeds max (5), escalate to parent's parent.
```

### Deferred Context Switch (not interrupt-level switching)

Timer interrupt sets a flag. Actual switch happens at function-call level after `iretq`. This prevents interrupt frame leaks that would overflow the stack after ~400 preemptions.

---

## Quick Start

### Prerequisites

- **Rust nightly** (installed automatically via `rust-toolchain.toml`)
- **QEMU:** `brew install qemu` (macOS) / `apt install qemu-system-x86` (Linux)
- **xorriso:** `brew install xorriso` (macOS) / `apt install xorriso` (Linux)

### Build & Run

```bash
git clone https://github.com/gkganesh12/oxide-os.git
cd oxide-os/oxide-os
make run
```

This builds the kernel, creates a bootable ISO, and launches QEMU with virtio networking. Serial output appears in your terminal.

### Commands

```bash
make run          # Boot with GUI + serial
make run-headless # Boot headless (serial only)
make test         # Automated boot test
make clean        # Clean build artifacts
```

---

## The Numbers

| Metric | Value |
|--------|-------|
| Total kernel code | 5,148 lines of Rust |
| Boot time (QEMU) | <1 second |
| Kernel memory footprint | ~7 MiB |
| Available RAM | 117 MiB (of 128 MiB) |
| Hardware subsystems | PCI, virtio-net, virtio-blk, APIC, UART |
| Kernel subsystems | 15 modules |
| Syscalls defined | 20 (11 implemented) |
| Context switch cost | 8 assembly instructions |
| Preemption rate | ~400 Hz |
| IPC capacity | 256 messages per mailbox |
| Capability operations | Create, delegate, revoke (cascading) |
| Agent demo | 4 agents, 25+ messages, 0 errors |

---

## Roadmap

All 10 original phases are complete. Current work:

| Feature | Status |
|---------|--------|
| PCI bus enumeration | Done |
| Real virtio-net driver (virtqueues) | Done |
| Real virtio-blk driver (virtqueues) | Done |
| HTTP over real TCP sockets | Done |
| Syscall instruction (MSRs + assembly) | Done |
| OxideFS persistence to disk | Next |
| Ring-3 user-space isolation | Planned |
| TLS 1.3 for HTTPS | Planned |
| SMP (multi-core) | Future |

---

## Documentation

| Document | Description |
|----------|-------------|
| [Design Spec](docs/superpowers/specs/2026-05-02-oxide-os-design.md) | Full architecture specification |
| [Technical Article](docs/ARTICLE.md) | Deep-dive into how it was built |
| [Implementation Plans](docs/superpowers/plans/) | Phase-by-phase implementation details |
| [Changelog](CHANGELOG.md) | All changes with architectural decisions |
| [Technical Debt](TODO.md) | Known issues and what's been resolved |

---

## Contributing

Oxide OS is open source under the MIT License.

```bash
git clone https://github.com/gkganesh12/oxide-os.git
cd oxide-os/oxide-os
make run
```

---

## License

MIT License — see [LICENSE](oxide-os/LICENSE)

---

*Built by [Ganesh Khetawat](https://github.com/gkganesh12). An operating system where AI agents are not guests — they're the reason the kernel exists.*
