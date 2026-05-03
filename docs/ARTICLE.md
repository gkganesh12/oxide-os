# Building an AI Agent Operating System from Scratch in Rust

*How I built Oxide OS — a microkernel designed for AI agent swarms, from bare metal boot to real hardware drivers, in ~5,000 lines of Rust.*

---

## The Problem

AI agents are everywhere — browsing the web, writing code, managing customer support. But they all run on general-purpose operating systems that were designed for humans, not autonomous programs.

When you deploy 50 AI agents on Linux:
- They share the same filesystem, network, and permissions
- A compromised agent can access everything
- No built-in communication between agents
- No supervision — if one crashes, nothing restarts it
- Docker helps, but it's a workaround, not a solution

**What if the operating system itself was built for AI agents?**

## The Architecture

Oxide OS is a hybrid microkernel where AI agents are first-class kernel primitives. Each agent gets:

- **Capability-based security** — unforgeable kernel tokens that control exactly what each agent can access
- **Built-in IPC** — message passing, shared memory, pub/sub channels
- **Supervision trees** — Erlang-style restart policies (one agent crashes → auto-restart)
- **Priority scheduling** — real-time agents get priority over background work
- **Per-agent storage** — isolated key-value context store

```
┌──────────────────────────────────────────────────────┐
│  Oxide OS — Agent-Native Microkernel                 │
├──────────────────────────────────────────────────────┤
│  Agent Layer     │ Spawn, kill, supervise, restart    │
│  IPC             │ Messages, shared mem, pub/sub      │
│  Capabilities    │ Create, delegate, revoke           │
│  Scheduler       │ Preemptive, 3 priorities, fair     │
│  Networking      │ TCP/IP, HTTP, capability firewall  │
│  Storage         │ OxideFS, per-agent context store   │
│  Crypto          │ RNG, HMAC-SHA256, agent signing    │
│  Hardware        │ PCI, virtio-net, APIC, UART        │
│  Boot            │ Limine → x86_64 bare metal         │
└──────────────────────────────────────────────────────┘
```

## What Makes It Real

This isn't a toy. The kernel boots on bare metal (via QEMU) and talks to real hardware:

### PCI Bus Discovery
```
[pci] Found 6 devices
[pci]   00:00.0 vendor=8086 device=1237 class=06:00  (Intel 440FX)
[pci]   00:03.0 vendor=1AF4 device=1000 class=02:00  (virtio-net)
```

The kernel scans the PCI configuration space, discovers devices by vendor/device ID, reads BARs, and enables bus mastering for DMA.

### Real Virtio-Net Driver
```
[net] Found virtio-net at PCI 00:03.0
[net] virtio-net I/O base: 0xC000
[net] Device features: 0x79BF8064
[net] MAC: 52:54:00:12:34:56
[net] Queue 0: 256 entries, phys=0x1B8000
[net] Queue 1: 256 entries, phys=0x1BB000
[net] virtio-net: ONLINE (real driver)
```

The driver implements the full virtio legacy PCI transport:
1. Device reset and feature negotiation
2. Split virtqueue setup (descriptor table + available ring + used ring)
3. 256 pre-posted RX buffers
4. Ring-based TX with descriptor reclamation

### Preemptive Multitasking
```
[researcher-1] Sent finding #1 to aggregator
[aggregator] Received finding #1: {"agent":"researcher-1","topic":"AI alignment"}
[researcher-2] Sent finding #1 to aggregator
[supervisor] Status #1: 4 agents, 4 tasks, tick=509
```

Tasks are preempted by an APIC timer interrupt. Context switch is done in 8 lines of assembly, saving and restoring callee-saved registers. Each task gets its own 16 KiB kernel stack with a guard page.

### Capability-Based Security
```
[cap] Created root #1 for task 1 -> AgentSpawn [D|SPAWN|KILL]
[cap] Created root #2 for task 2 -> Agent(4) [W]
```

Every resource access goes through a capability check. Agent 2 has a WRITE capability to Agent 4 — it can send messages to the aggregator but cannot spawn new agents, access storage, or connect to the network. Capabilities can be delegated (subset only) and revoked (cascading to all children).

## The Numbers

| Metric | Value |
|--------|-------|
| Total kernel code | ~5,000 lines of Rust |
| Boot time (QEMU) | <1 second |
| Memory used | 7 MiB (of 128 MiB available) |
| Subsystems | 13 (PCI, virtio, net, storage, crypto, etc.) |
| Syscalls defined | 20 |
| Context switch | ~8 assembly instructions |
| Preemption rate | ~400 Hz (APIC timer) |

## The 10 Phases

| Phase | What | Lines |
|-------|------|-------|
| 1. Boot | Limine, GDT, IDT, memory, heap | 400 |
| 2. Scheduler | APIC timer, context switch, priority queues | 500 |
| 3. Capabilities | Unforgeable tokens, delegation, revocation | 380 |
| 4. IPC | Messages, shared memory, pub/sub, request/reply | 470 |
| 5. Agents | Spawn, kill, supervision trees, restart policies | 340 |
| 6. Networking | smoltcp TCP/IP, HTTP, DNS, capability firewall | 370 |
| 7. Storage | OxideFS, block cache, per-agent context store | 315 |
| 8. Crypto | Hardware RNG, HMAC-SHA256, agent signing | 440 |
| 9. Syscalls | 20 calls, ELF loader, process management | 265 |
| 10. GPU | Inference request scheduler with deadlines | 160 |
| **PCI + virtio** | Real hardware driver | 540 |

## Key Design Decisions

### Why Rust?
Memory safety without runtime cost. In a kernel, a use-after-free is a security vulnerability. Rust's ownership system prevents entire classes of bugs at compile time. We use `unsafe` only at hardware boundaries (port I/O, page table manipulation, inline assembly).

### Why capabilities instead of UNIX permissions?
UNIX permissions are coarse (read/write/execute on files). Capabilities are fine-grained:
- `net:api.openai.com:443` — can ONLY connect to OpenAI
- `storage:write:/agents/2/memory` — can ONLY write to its own context store
- `agent:spawn` — can spawn children, but only with a SUBSET of its own capabilities

An agent with zero capabilities can do nothing. There is no ambient authority.

### Why supervision trees?
AI agents fail. Models hallucinate. API calls timeout. Network drops. The question isn't "will agents fail?" but "what happens when they do?"

Oxide OS uses Erlang-style supervision:
- **RestartOne** — restart just the failed agent
- **RestartAll** — restart all siblings (for interdependent agents)
- **Escalate** — kill the parent, let the grandparent decide
- **Permanent** — never restart (one-shot agents)

### Why a deferred context switch?
The timer interrupt sets a flag. The actual context switch happens AFTER the interrupt returns (via `iretq`). This avoids leaking interrupt frames on the stack — a subtle bug that would cause a stack overflow after ~400 preemptions.

## What's Next

The kernel foundation is complete. The next steps to make this a deployable product:

1. **Full virtio packet flow** — connect the virtqueue RX/TX to smoltcp so TCP connections work end-to-end
2. **Real HTTP requests** — agents calling OpenAI/Anthropic APIs from bare metal
3. **virtio-blk driver** — persistent storage that survives reboot
4. **Ring-3 user space** — run agent code in isolated user-mode processes
5. **CLI and API server** — `oxide agent spawn --model gpt-4 --task "research X"`

## Try It

```bash
git clone https://github.com/gkganesh12/oxide-os.git
cd oxide-os/oxide-os
make run
```

Requires: Rust nightly, QEMU, xorriso.

## Source

**GitHub:** [github.com/gkganesh12/oxide-os](https://github.com/gkganesh12/oxide-os)
**License:** MIT

---

*Oxide OS is built by Ganesh Khetawat. The kernel boots in under a second, manages AI agent swarms with capability-based security, and talks to real hardware via PCI and virtio. All in ~5,000 lines of Rust.*
