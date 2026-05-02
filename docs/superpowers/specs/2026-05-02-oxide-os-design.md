# Oxide OS — Design Specification

## Overview

Oxide OS is an agent-native hybrid microkernel operating system written in Rust, purpose-built for running AI agent swarms. It treats AI agents as first-class kernel primitives with capability-based security, multiple IPC mechanisms, and integrated support for both local model inference and remote LLM API access.

**Target:** Hypervisor-first (QEMU/KVM), with a path to bare metal.
**Language:** Rust (`#![no_std]` kernel, standard Rust for user-space services).
**Philosophy:** Agents are the primary workload. Humans interact via Web UI and CLI. Easy to deploy — boot an image and start orchestrating agents.

---

## MVP Scope

Multiple agents running concurrently with:
- IPC (message passing, shared memory)
- Preemptive scheduling with priority classes
- Capability-based resource isolation
- Basic networking (HTTP client for LLM API calls)
- CLI for agent management

## Full Vision

- Local model inference with GPU/NPU scheduling
- Tool sandboxing via WASM
- Persistent agent memory/context store
- Pub/sub event channels
- Web dashboard for monitoring and management
- REST/gRPC API for programmatic control
- Supervision trees with restart policies
- Content-addressable filesystem (OxideFS)

---

## Architecture

### Kernel (Hybrid Microkernel)

The kernel runs performance-critical services in kernel space while keeping drivers and application logic in user-space.

#### Kernel Subsystems

| Subsystem | Responsibility |
|-----------|---------------|
| Scheduler | Preemptive, priority-based. Scheduling classes: `realtime` (inference deadlines), `normal` (agent orchestration), `background` (maintenance). |
| Memory Manager | Virtual address spaces per process, shared memory grants (zero-copy), physical page allocator, MMIO for device access. |
| Capability System | All resource access mediated by capability tokens. Supports: creation, delegation (subset), revocation, validation on every IPC boundary. No ambient authority. |
| IPC | Synchronous message passing (fast path), asynchronous channels, shared memory regions, request/reply with timeouts. |
| Networking | Full TCP/IP stack, async socket API, built-in HTTP/HTTPS client, DNS resolver. |
| Storage | Block device layer (virtio-blk), log-structured filesystem (OxideFS), per-agent key-value context store. |
| Agent Lifecycle | Spawn, kill, monitor, restart. Resource accounting per agent. Supervision tree management. |
| Timers & Clocks | High-precision timers, deadline scheduling for inference requests. |
| Crypto | TLS 1.3, Ed25519 signing, capability token HMAC validation, secure RNG (hardware-backed when available). |
| Interrupt Routing | Minimal interrupt handling, dispatch to kernel subsystems or user-space drivers. |
| GPU/NPU Scheduler | Time-slicing accelerator access between agents. Priority queue for inference requests. |

#### User-Space Services

| Service | Responsibility |
|---------|---------------|
| Device Drivers | virtio-net, virtio-blk, GPU drivers. Isolated — crash doesn't take down kernel. |
| Model Server | Loads and serves local models (GGML/ONNX). Receives inference requests via IPC. |
| Tool Sandbox | WASM runtime for executing agent tools in isolation. |
| Web Dashboard | Real-time monitoring UI, agent management, log viewer. |
| CLI (`oxide`) | Command-line interface for agent management and system control. |
| API Server | REST/gRPC endpoint for programmatic control and external integration. |
| Supervisor Agent | Root of the agent supervision tree. Orchestrates system-level agent management. |

---

## Agent Model

An Agent is a kernel-managed entity with:

### Agent Properties

| Property | Description |
|----------|-------------|
| Identity | Unique `AgentID` (u64) + Ed25519 keypair generated at spawn. |
| Capability Set | Immutable at spawn (parent grants). Defines what the agent can access. |
| State | `Idle` / `Running` / `Waiting` / `Suspended` / `Dead` |
| Memory Space | Private virtual address space + optional shared memory grants. |
| Context Store | Kernel-managed persistent KV store (conversation history, working memory). Survives restarts. |
| Tool Registry | Capabilities to specific tools (WASM sandboxes) this agent can invoke. |
| Parent/Children | Tree structure. Parent delegates capabilities to children. |
| Model Binding | Which models (local or remote) this agent can use. Via capability. |
| Restart Policy | `restart-one`, `restart-all`, `escalate` (Erlang-style supervision). |

### Agent Lifecycle

```
Spawn(config, capabilities)
  → Init (load system prompt, connect to model endpoint)
  → Running (perceive → decide → act loop)
  → Communicate (IPC: messages, pubsub, shared memory)
  → Delegate (spawn children with capability subsets)
  → Die / Restart (kernel cleanup, notify parent, apply restart policy)
```

### Agent Configuration (at spawn)

```rust
struct AgentConfig {
    name: String,
    system_prompt: Option<String>,
    model: ModelBinding,          // Local model ID or remote endpoint
    tools: Vec<ToolCapability>,
    capabilities: CapabilitySet,
    restart_policy: RestartPolicy,
    resource_limits: ResourceLimits,
    context_store: bool,          // Enable persistent memory
}
```

---

## IPC & Communication

### Mechanisms

| Mechanism | Semantics | Use Case |
|-----------|-----------|----------|
| Message Passing | Typed, async, kernel-buffered. Address by AgentID or capability handle. | General agent-to-agent coordination. |
| Shared Memory | Kernel grants region. Both agents get capability to it. Zero-copy. | Large data: model weights, embeddings, documents. |
| Pub/Sub Channels | Named channels. Subscribe with capability. Kernel broadcasts. | Swarm events: "task available", "model loaded". |
| Request/Reply | Synchronous IPC with timeout. Caller blocks until response or deadline. | Tool invocation, service calls. |
| Capability Transfer | Send capability token via message. Kernel validates and transfers ownership. | Delegating access to resources or agents. |

### Message Format

```rust
struct Message {
    sender: AgentID,
    recipient: AgentID,
    msg_type: MessageType,
    payload: Vec<u8>,        // Serialized (MessagePack or similar)
    capability: Option<Capability>,  // Optional cap transfer
    reply_to: Option<MessageID>,
}
```

---

## Capability System

### Design

- Every resource (memory region, network endpoint, agent handle, tool, model, channel) is accessed via a capability token.
- Capabilities are unforgeable kernel objects (index into kernel capability table).
- No ambient authority — an agent with zero capabilities can do nothing.
- Delegation: agent can create a derived capability with equal or fewer permissions.
- Revocation: parent can revoke any capability it delegated (cascading to children).

### Capability Structure

```rust
struct Capability {
    id: CapID,
    resource: ResourceRef,       // What this cap points to
    permissions: PermissionBits, // Read, Write, Execute, Delegate, etc.
    owner: AgentID,
    parent_cap: Option<CapID>,   // For revocation chain
    expiry: Option<Timestamp>,   // Optional time-limited caps
}
```

### Permission Examples

| Capability | Grants |
|------------|--------|
| `net:api.openai.com:443` | HTTPS access to OpenAI API only |
| `inference:llama3-70b` | Submit inference requests to local Llama 3 model |
| `agent:spawn` | Permission to spawn child agents |
| `pubsub:research-findings` | Subscribe/publish to the research-findings channel |
| `storage:write:/data/reports` | Write access to a specific storage path |
| `tool:web-browser` | Invoke the web-browser tool sandbox |

---

## Local Inference Engine

### Components

| Component | Location | Responsibility |
|-----------|----------|---------------|
| GPU/NPU Scheduler | Kernel | Time-slice accelerator between agents. Priority queue with deadlines. |
| Weight Memory Manager | Kernel | Shared read-only memory regions for model weights. Multiple agents share one loaded model. |
| Model Server | User-space | Loads models (GGML, ONNX), handles inference requests, manages model lifecycle. |
| Inference Queue | Kernel | Priority queue. Agents submit requests with deadlines. Kernel schedules. |

### Unified Interface

Agents don't distinguish between local and remote models:

```rust
enum ModelBinding {
    Local { model_id: String },
    Remote { endpoint: Url, api_key_cap: CapID },
    Auto { preference: Vec<String> },  // Try local first, fallback to remote
}
```

A routing layer resolves `Auto` bindings — checks if model is loaded locally, otherwise routes to remote API via kernel HTTP client.

---

## Tool Sandbox

### Design

- Tools are WASM modules registered in the kernel's tool registry.
- Agents invoke tools via Request/Reply IPC.
- Each tool invocation runs in a fresh WASM sandbox with:
  - CPU time limit
  - Memory limit
  - Network access only if tool's capability allows it
  - Filesystem access only via granted capabilities

### Tool Interface

```rust
trait Tool {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Schema;
    fn execute(&self, input: Value, caps: &[Capability]) -> Result<Value, ToolError>;
}
```

### Built-in Tools (shipped with Oxide OS)

- `web-fetch` — HTTP GET/POST with capability-gated URLs
- `code-exec` — Execute code snippets in sandboxed WASM
- `file-read` / `file-write` — Storage access via capabilities
- `shell` — Restricted command execution (capability-gated)

---

## Storage & Persistence

### Layers

| Layer | Location | Purpose |
|-------|----------|---------|
| Block Layer | Kernel | Raw virtio-blk access, block caching. |
| OxideFS | Kernel | Log-structured, append-friendly, content-addressable filesystem. Optimized for agent workloads (lots of small writes, context snapshots). |
| Agent Context Store | Kernel | Per-agent persistent KV store. Fast reads/writes. Survives agent restarts. Used for conversation history, working memory, learned patterns. |
| Object Store | User-space | Higher-level storage for documents, embeddings, model files. Capability-gated access. |

### OxideFS Properties

- Log-structured: append-only writes, garbage collection in background
- Content-addressable: deduplicate shared data (common embeddings, repeated context)
- Snapshot support: instant agent state snapshots for rollback/debugging
- Optimized for small-to-medium objects (messages, context chunks, tool outputs)

---

## Networking

| Component | Location | Description |
|-----------|----------|-------------|
| TCP/IP Stack | Kernel | Full stack, async I/O, high connection count support. |
| HTTP/HTTPS Client | Kernel | Built-in client for LLM API calls. TLS via kernel crypto. |
| DNS Resolver | Kernel | Caching resolver. |
| Firewall / Policy | Kernel | Capability-gated. Agents can ONLY reach endpoints their capability specifies. `net:*` for unrestricted (system agents only). |
| API Server | User-space | REST/gRPC. Listens on management port. Exposes agent CRUD, system status, logs. |

---

## Management Layer

### Supervisor Agent

- First agent spawned by kernel at boot.
- Holds all capabilities (root of delegation tree).
- Manages the top-level supervision tree.
- Receives commands from CLI / Web UI / API Server.
- Human-facing: can accept natural language instructions and translate to agent operations.

### CLI (`oxide`)

```bash
oxide agent spawn --name researcher --model auto --tools web-fetch --prompt "Research AI safety papers"
oxide agent list
oxide agent logs researcher
oxide agent kill researcher
oxide status
oxide model load llama3-70b --path /models/llama3-70b.gguf
oxide model list
oxide channel list
oxide config set default-model gpt-4
```

### Web Dashboard

- Real-time agent status (running, idle, waiting, dead)
- Supervision tree visualization
- Message flow / IPC visualization
- Resource usage per agent (CPU, memory, network, inference time)
- Log viewer with filtering
- Agent spawn/configure UI
- Model management (load, unload, monitor)

### API Server

- REST + gRPC on configurable port
- Auth via API keys or mTLS
- Endpoints: `/agents`, `/models`, `/channels`, `/system`, `/logs`
- WebSocket endpoint for real-time status streaming to dashboard

---

## Boot Sequence

1. Bootloader (UEFI/BIOS via `bootloader` crate or custom) loads kernel ELF
2. Kernel initializes: memory, interrupts, scheduler, capability table
3. Kernel initializes subsystems: networking, storage, timers, crypto, GPU scheduler
4. Kernel mounts OxideFS on root block device
5. Kernel spawns Init Server (first user-space process)
6. Init Server spawns core services: Device Manager, Model Server, Tool Sandbox, API Server, Web Dashboard
7. Init Server spawns Supervisor Agent (root of agent tree)
8. Supervisor Agent loads config, spawns initial agent swarm
9. System ready — accepting CLI / Web UI / API commands

---

## Target Platforms

### MVP (Hypervisor)
- x86_64 on QEMU/KVM
- virtio devices (virtio-net, virtio-blk, virtio-gpu)
- Serial console for early debug

### Future (Bare Metal)
- x86_64 with real hardware drivers
- ARM64 (Raspberry Pi, cloud ARM instances)
- Real GPU drivers (initially: NVIDIA via user-space driver server)

---

## Technology Choices

| Component | Technology |
|-----------|-----------|
| Language | Rust (kernel: `#![no_std]`, user-space: standard) |
| Bootloader | `bootloader` crate or Limine |
| Async Runtime | Custom kernel async executor (no tokio in kernel) |
| Serialization | MessagePack (compact, fast) for IPC payloads |
| WASM Runtime | Wasmtime (user-space tool sandbox) |
| Inference | `llama.cpp` bindings or `candle` (Rust-native ML) |
| Web Dashboard | Lightweight SPA (served by API server) |
| Build System | Cargo workspace, custom build scripts for kernel image |
| Testing | QEMU-based integration tests, unit tests per subsystem |

---

## Non-Goals (Explicitly Out of Scope)

- Desktop GUI / window manager
- POSIX compatibility
- Running arbitrary Linux binaries
- Multi-user login system
- Package manager for end-user apps
- Backwards compatibility with existing OS interfaces
