# Oxide OS

An agent-native microkernel operating system written in Rust, purpose-built for running AI agent swarms.

## What is Oxide OS?

Oxide OS treats AI agents as first-class kernel primitives. Instead of running agents in containers on top of a general-purpose OS, Oxide OS provides:

- **Capability-based security** — agents can only access what they're explicitly granted. No ambient authority.
- **Agent lifecycle management** — spawn, kill, monitor, and restart agents as kernel operations.
- **Multiple IPC mechanisms** — message passing, shared memory, pub/sub, request/reply — chosen per task.
- **Supervision trees** — Erlang-style restart policies for fault-tolerant agent swarms.
- **Local + remote inference** — unified interface for both local models and cloud LLM APIs.
- **Tool sandboxing** — WASM-based isolated execution for agent tools.

## Current Status

**v1.0.0 — All 10 phases complete:**

```
  ╔══════════════════════════════════════╗
  ║        Oxide OS v1.0.0               ║
  ║   Agent-Native Microkernel (Rust)    ║
  ╚══════════════════════════════════════╝

[boot] Limine protocol OK | GDT | IDT
[memory] 30097/32582 frames free (117 MiB)
[net] TCP/IP 10.0.2.15/24 | DNS | HTTP | Firewall
[storage] OxideFS | Block cache | Context store
[crypto] RNG | HMAC-SHA256 | Agent signing
[timer] System clock | Deadline queue
[gpu] Inference scheduler (priority + deadlines)
[syscall] 20 calls registered
[userspace] ELF loader | Process management

  All 10 subsystems operational

[demo] Agent supervision tree:
|- supervisor [Running]
  |- researcher-1 [Running]
  |- researcher-2 [Running]
  |- aggregator [Running]

[researcher-1] Sent finding #1 → aggregator
[aggregator] Received: {"agent":"researcher-1","topic":"AI alignment"}
[supervisor] Status: 4 agents, 4 tasks, tick=509
```

### What works today

**Phase 1 — Kernel Foundation:**
- Boots in QEMU via Limine bootloader (BIOS + UEFI)
- Serial console output (deadlock-free panic printing)
- GDT with TSS (page-aligned double-fault stack)
- IDT handling CPU exceptions (page fault, GPF, double fault, invalid opcode)
- Bitmap physical frame allocator with kernel memory protection
- Virtual memory (4-level page tables via HHDM)
- 1 MiB kernel heap (`Vec`, `String`, `Box`, `BTreeMap`, etc.)
- OOM handler with clear error reporting

**Phase 2 — Preemptive Scheduler:**
- Local APIC with periodic timer interrupt (~400 Hz)
- Multi-level priority scheduler (Realtime > Normal > Background)
- Preemptive multitasking via timer-driven context switching
- Per-task kernel stacks (16 KiB, contiguous, with guard page)
- Naked-function context switch (callee-saved register save/restore)
- Fair scheduling: all tasks get CPU time regardless of priority
- Task lifecycle: spawn, block, unblock, kill, yield
- Deferred-switch model (no interrupt frame leaks)
- Entry trampoline for clean task startup with interrupts enabled

## Building

### Prerequisites

- Rust nightly (installed automatically via `rust-toolchain.toml`)
- QEMU: `brew install qemu` (macOS) or `apt install qemu-system-x86` (Linux)
- xorriso: `brew install xorriso` (macOS) or `apt install xorriso` (Linux)

### Build & Run

```bash
cd oxide-os
make run
```

This will:
1. Build the kernel for `x86_64-unknown-none`
2. Clone and build the Limine bootloader (first time only)
3. Create a bootable ISO
4. Launch QEMU with serial output to your terminal

### Other commands

```bash
make kernel   # Build kernel only
make iso      # Build bootable ISO
make test     # Run in QEMU with debug exit device (automated testing)
make clean    # Clean build artifacts
```

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  User Space                      │
│  ┌──────────┐ ┌──────────┐ ┌────────────────┐  │
│  │ CLI      │ │ API      │ │ Web Dashboard  │  │
│  │ (oxide)  │ │ Server   │ │ (:8081)        │  │
│  └──────────┘ └──────────┘ └────────────────┘  │
│  ┌──────────┐ ┌──────────┐ ┌────────────────┐  │
│  │ Model    │ │ Tool     │ │ Device Drivers │  │
│  │ Server   │ │ Sandbox  │ │ (virtio)       │  │
│  └──────────┘ └──────────┘ └────────────────┘  │
├─────────────────────────────────────────────────┤
│              Kernel (Hybrid Microkernel)         │
│                                                 │
│  Scheduler │ Memory │ IPC │ Capabilities        │
│  Networking │ Storage │ Crypto │ Timers          │
│  Agent Lifecycle │ GPU Scheduler                 │
└─────────────────────────────────────────────────┘
```

### Kernel Module Structure

```
oxide-os/kernel/src/
├── main.rs              # Boot sequence, task entry points
├── serial.rs            # UART 16550 driver (deadlock-free panic output)
├── gdt.rs               # Global Descriptor Table + TSS
├── interrupts.rs        # IDT, exception handlers, timer interrupt
├── apic.rs              # Local APIC driver (timer, EOI, MMIO mapping)
├── allocator.rs         # Kernel heap (linked-list, 1 MiB)
├── qemu.rs              # QEMU debug exit device
├── memory/
│   ├── mod.rs           # Memory subsystem constants
│   ├── frame_allocator.rs  # Bitmap physical frame allocator
│   └── paging.rs        # Page table management (OffsetPageTable)
└── task/
    ├── mod.rs           # Task struct, stack allocation, entry trampoline
    ├── context.rs       # CpuContext + naked context_switch assembly
    └── scheduler.rs     # Priority queue scheduler, yield, block/unblock
```

## Roadmap

| Phase | Status | Description |
|-------|--------|-------------|
| 1. Kernel Boot & Foundation | Done | Limine, GDT, IDT, memory, heap |
| 2. Scheduler & Interrupts | Done | Preemptive multi-priority, context switch |
| 3. Capability System | Done | Unforgeable tokens, delegation, revocation |
| 4. IPC | Done | Messages, shared memory, pub/sub, request/reply |
| 5. Agent Lifecycle | Done | Spawn, kill, supervision trees, restart policies |
| 6. Networking | Done | TCP/IP (smoltcp), HTTP, DNS, capability firewall |
| 7. Storage | Done | OxideFS, block cache, per-agent context store |
| 8. Crypto & Timers | Done | RNG, HMAC-SHA256, signing, deadline queue |
| 9. User-Space & Management | Done | 20 syscalls, ELF loader, process management |
| 10. Inference & GPU | Done | Priority-based inference scheduler with deadlines |

## Design

- Full design specification: [`docs/superpowers/specs/2026-05-02-oxide-os-design.md`](docs/superpowers/specs/2026-05-02-oxide-os-design.md)
- Implementation plans: [`docs/superpowers/plans/`](docs/superpowers/plans/)

## Contributing

Oxide OS is open source under the MIT License. Contributions welcome.

```bash
# Clone and build
git clone https://github.com/gkganesh12/oxide-os.git
cd oxide-os/oxide-os
make run
```

## License

MIT License — see [LICENSE](oxide-os/LICENSE)
