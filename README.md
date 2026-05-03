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

**Phase 1 complete** — kernel foundation operational:

```
  ╔══════════════════════════════════════╗
  ║        Oxide OS v0.1.0               ║
  ║   Agent-Native Microkernel (Rust)    ║
  ╚══════════════════════════════════════╝

[boot] Limine protocol OK
[boot] GDT loaded
[boot] IDT loaded
[memory] Frame allocator: 31557/32581 frames free (123 MiB free)
[boot] Page tables ready
[heap] Kernel heap initialized: 1024 KiB (256 pages)

[boot] Phase 1 complete — kernel foundation operational
```

### What works today
- Boots in QEMU via Limine bootloader (BIOS + UEFI)
- Serial console output
- GDT with TSS (double-fault stack)
- IDT handling CPU exceptions (page fault, GPF, double fault)
- Bitmap physical frame allocator
- Virtual memory (4-level page tables via HHDM)
- 1 MiB kernel heap (Vec, String, Box, BTreeMap, etc.)

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
make test     # Run in QEMU with debug exit device
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

## Roadmap

| Phase | Status | Description |
|-------|--------|-------------|
| 1. Kernel Boot & Foundation | Done | Boot, memory, heap |
| 2. Scheduler & Interrupts | Next | Preemptive multi-priority scheduling |
| 3. Capability System | Planned | Unforgeable tokens, delegation, revocation |
| 4. IPC | Planned | Messages, shared memory, pub/sub |
| 5. Agent Lifecycle | Planned | Agents as kernel entities, supervision |
| 6. Networking | Planned | TCP/IP, HTTP client for LLM APIs |
| 7. Storage | Planned | OxideFS, per-agent context store |
| 8. Crypto & Timers | Planned | TLS, signing, deadline scheduling |
| 9. User-Space & Management | Planned | Syscalls, CLI, REST API |
| 10. Inference & Dashboard | Planned | Local models, WASM tools, web UI |

## Design

Full design specification: [`docs/superpowers/specs/2026-05-02-oxide-os-design.md`](docs/superpowers/specs/2026-05-02-oxide-os-design.md)

## License

MIT License — see [LICENSE](oxide-os/LICENSE)
