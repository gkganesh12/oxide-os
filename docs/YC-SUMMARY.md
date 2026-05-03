# Oxide OS — YC Application Technical Summary

## One-liner
An operating system built from scratch in Rust where AI agents are kernel primitives — not containers on top of Linux.

## The Problem
Enterprises want to deploy autonomous AI agents but can't. Security teams block it because there's no way to control what agents access. Today's stack (Linux + Docker + Redis + iptables) was designed for humans, not autonomous programs. Every piece of agent infrastructure is a workaround bolted onto an OS that doesn't understand agents.

## The Solution
Oxide OS is a microkernel where agents are the reason the kernel exists:

- **Capability-based security** — each agent gets unforgeable kernel tokens controlling exactly what it can access (e.g., `net:api.openai.com:443`). Zero ambient authority.
- **Built-in IPC** — message passing, shared memory, pub/sub, request/reply. No Redis needed.
- **Supervision trees** — Erlang-style crash resilience. Agent dies → kernel auto-restarts it with the right policy.
- **Real hardware drivers** — PCI bus enumeration, virtio-net/blk with DMA ring buffers. This is a real OS, not a simulator.

## What's Built (today, working, open source)
- 5,384 lines of Rust kernel code, 0 lines of C
- Boots in <1 second on QEMU (x86_64 bare metal)
- Real PCI discovery + virtio-net driver with 256-entry split virtqueues
- Real virtio-blk driver with virtqueue block I/O
- Preemptive multi-priority scheduler with context switching (8 asm instructions)
- Capability system: create, delegate (subset only), revoke (cascading)
- 4 IPC mechanisms: messages, shared memory, pub/sub channels, request/reply
- Agent lifecycle: spawn, kill, supervision trees with restart policies
- TCP/IP networking via smoltcp, HTTP client with real TCP socket flow
- OxideFS: log-structured filesystem with content-addressable dedup
- HMAC-SHA256 crypto, hardware RNG, per-agent signing
- 20 syscalls with x86_64 MSR wiring (LSTAR, STAR, SFMASK)
- GitHub Actions CI with automated boot testing

## Demo
The kernel boots and runs an agent swarm:
- Supervisor agent spawns 2 researchers + 1 aggregator
- Researchers produce findings via capability-gated IPC
- Aggregator collects and summarizes
- All communication enforced by kernel capability tokens
- Zero errors over 2000+ timer ticks

## Business
- **Target:** Companies deploying AI agents at scale (customer support, coding, research)
- **Moat:** Kernel-level capability security — can't be replicated by a Docker plugin
- **GTM:** Open source kernel → managed cloud (Oxide Cloud) → enterprise on-prem
- **Pricing:** Free (OSS) → $0.01/agent-hour (cloud) → $50K+/year (enterprise)

## What's Next (3 months)
1. TLS 1.3 → agents call real LLM APIs from bare metal
2. Ring-3 user-space → full process isolation
3. CLI: `oxide agent spawn --model gpt-4 --task "research X"`
4. First external user running agents on Oxide OS

## Links
- **GitHub:** https://github.com/gkganesh12/oxide-os
- **Landing page:** (deploy URL here)
