# Kernel → Host Broker Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the simulated agent loop in `oxide-os/kernel/src/swarmscope/loader.rs` so kernel agents POST to the host broker (PR #13, port 3101) over virtio-net and publish real Claude replies onto per-agent pub/sub channels for the aggregator to collect.

**Architecture:** Each individual/group_rep agent is auto-granted a network capability targeting the broker (default `10.0.2.2:3101`, QEMU SLIRP gateway) and a publish capability on `agent.<name>.out`. The aggregator is auto-granted subscribe on each `agent.*.out` channel. The agent entry function looks itself up in the registry, builds a JSON POST body, calls `net::http::post`, parses the reply, and publishes a structured payload to its channel. The aggregator drains its mailbox (channel pub/sub delivers into the subscriber's existing mailbox) and logs each message.

**Tech Stack:** Rust 2024 edition, no_std bare-metal kernel, alloc only. smoltcp 0.12 for networking (already wired through `net::http::post`). No new crate dependencies. Verification = `cargo build` (compile-check) + manual QEMU boot against running broker.

**Spec:** `docs/superpowers/specs/2026-05-31-kernel-broker-wiring-design.md` (committed in this branch).

---

## File Structure

```
oxide-os/kernel/src/
├── agent/
│   └── registry.rs           # MODIFY: add 2 fields to Agent struct, default-init helpers
├── net/
│   └── http.rs               # MODIFY: bump recv_deadline 1000 → 5000 in post()
└── swarmscope/
    ├── mod.rs                # MODIFY: add `pub mod broker;`
    ├── manifest.rs           # MODIFY: add broker_endpoint to SwarmManifest + builder
    ├── broker.rs             # NEW: BrokerError, InferReply, post_infer(), JSON helpers
    └── loader.rs             # MODIFY: rewrite agent + aggregator entries, add 2 helpers
```

**Boundaries:**
- `broker.rs` is a pure helper: takes endpoint + name + message + caps, returns parsed reply. No globals.
- `loader.rs` owns the orchestration: when to call broker, what to publish, channel topology setup.
- `registry.rs` change is purely additive (two new fields); existing callsites only need a default value at construction.
- `manifest.rs` adds one optional field; builder gets one new method.

**Existing primitives we use unchanged:** `net::http::post`, `capability::CAP_TABLE.create_root`, `ipc::channel::create / subscribe / publish`, `ipc::message::receive`, `agent::registry::REGISTRY.get_mut`.

**Verification model:**
- Kernel has no `cargo test` (bare-metal, no_std). Each task's "verify" step is `cargo build` from `oxide-os/`, which must succeed.
- The final task (Task 9) is a full integration test: boot the kernel in QEMU against the running broker and inspect serial output.

---

## Task 1: Add `broker.rs` module (compile-only)

**Files:**
- Create: `oxide-os/kernel/src/swarmscope/broker.rs`
- Modify: `oxide-os/kernel/src/swarmscope/mod.rs`

- [ ] **Step 1: Create the broker module**

Create `oxide-os/kernel/src/swarmscope/broker.rs`:

```rust
//! Host-broker client.
//!
//! Pure helper that wraps `net::http::post` to call the host-side broker
//! (Node.js service from PR #13). One call = one Claude inference round.
//! No global state. Capability-checked at the HTTP layer.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::capability::CapId;
use crate::net::http::{self, HttpError};
use crate::task::TaskId;

pub const DEFAULT_BROKER_ENDPOINT: &str = "http://10.0.2.2:3101";

#[derive(Debug)]
pub enum BrokerError {
    Http(HttpError),
    Non2xx(u16),
    ParseFailure,
}

#[derive(Debug)]
pub struct InferReply {
    pub text: String,
    pub tokens_in: u32,
    pub tokens_out: u32,
}

/// POST {endpoint}/v1/agent/{agent_name}/infer with body {"message": <user_message>}.
///
/// Returns the parsed reply text and token usage on success.
pub fn post_infer(
    endpoint: &str,
    agent_name: &str,
    user_message: &str,
    task_id: TaskId,
    net_cap: CapId,
) -> Result<InferReply, BrokerError> {
    let url = format!("{}/v1/agent/{}/infer", endpoint, agent_name);
    let body = build_body(user_message);

    let resp = http::post(&url, body.as_bytes(), task_id, net_cap)
        .map_err(BrokerError::Http)?;

    if !(200..300).contains(&resp.status_code) {
        return Err(BrokerError::Non2xx(resp.status_code));
    }

    let body_str = core::str::from_utf8(&resp.body).map_err(|_| BrokerError::ParseFailure)?;
    parse_reply(body_str).ok_or(BrokerError::ParseFailure)
}

/// Build `{"message":"<escaped>"}` JSON. Hand-rolled to avoid pulling in serde.
fn build_body(message: &str) -> String {
    let mut out = String::with_capacity(message.len() + 16);
    out.push_str("{\"message\":\"");
    for ch in message.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push_str("\"}");
    out
}

/// Extract `text` (string) and `usage.input_tokens` / `usage.output_tokens` (numbers)
/// from the broker's JSON response. Tolerates extra fields and arbitrary whitespace
/// but does NOT support escaped characters inside `text`.
fn parse_reply(body: &str) -> Option<InferReply> {
    let text = extract_string_field(body, "\"text\"")?;
    let tokens_in = extract_number_field(body, "\"input_tokens\"").unwrap_or(0);
    let tokens_out = extract_number_field(body, "\"output_tokens\"").unwrap_or(0);
    Some(InferReply { text, tokens_in, tokens_out })
}

fn extract_string_field(body: &str, key: &str) -> Option<String> {
    let key_pos = body.find(key)?;
    let after_key = &body[key_pos + key.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    let rest = after_colon.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(String::from(&rest[..end]))
}

fn extract_number_field(body: &str, key: &str) -> Option<u32> {
    let key_pos = body.find(key)?;
    let after_key = &body[key_pos + key.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    let end = after_colon.find(|c: char| !c.is_ascii_digit())?;
    after_colon[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn build_body_escapes_quotes() {
        assert_eq!(build_body("he said \"hi\""), r#"{"message":"he said \"hi\""}"#);
    }
    #[test]
    fn parse_reply_extracts_text_and_usage() {
        let body = r#"{"agent":"alice","text":"hello","usage":{"input_tokens":12,"output_tokens":7}}"#;
        let reply = parse_reply(body).unwrap();
        assert_eq!(reply.text, "hello");
        assert_eq!(reply.tokens_in, 12);
        assert_eq!(reply.tokens_out, 7);
    }
}
```

- [ ] **Step 2: Export from mod.rs**

Edit `oxide-os/kernel/src/swarmscope/mod.rs`. The file currently contains only two lines:

```rust
pub mod manifest;
pub mod loader;
```

Add one line so it reads:

```rust
pub mod manifest;
pub mod loader;
pub mod broker;
```

- [ ] **Step 3: Verify compile**

Run from `/Users/ganeshkhetawat/My OS/oxide-os`:

```bash
cargo build 2>&1 | tail -20
```

Expected: `Finished ... target(s) in N.NNs` with no errors. Warnings about unused functions in `broker` are acceptable (loader doesn't call them yet).

- [ ] **Step 4: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add oxide-os/kernel/src/swarmscope/broker.rs oxide-os/kernel/src/swarmscope/mod.rs && git commit -m "feat(kernel): add swarmscope::broker module with post_infer"
```

---

## Task 2: Add `broker_endpoint` to `SwarmManifest` + builder

**Files:**
- Modify: `oxide-os/kernel/src/swarmscope/manifest.rs`

- [ ] **Step 1: Add field to SwarmManifest struct**

Find the `SwarmManifest` struct (around line 126 of `oxide-os/kernel/src/swarmscope/manifest.rs`). It currently looks like:

```rust
pub struct SwarmManifest {
    pub agents: Vec<ManifestAgent>,
    pub channels: Vec<String>,
    pub supervision: ManifestSupervision,
    pub project_id: Option<String>,
    pub graph_id: Option<String>,
    pub agent_count: usize,
}
```

Add one field at the end:

```rust
pub struct SwarmManifest {
    pub agents: Vec<ManifestAgent>,
    pub channels: Vec<String>,
    pub supervision: ManifestSupervision,
    pub project_id: Option<String>,
    pub graph_id: Option<String>,
    pub agent_count: usize,
    pub broker_endpoint: Option<String>,
}
```

- [ ] **Step 2: Add field to ManifestBuilder struct**

In the same file, find `pub struct ManifestBuilder` (around line 140). It has the same shape as SwarmManifest minus `agent_count` and (currently) without `broker_endpoint`. Add the new field:

```rust
pub struct ManifestBuilder {
    agents: Vec<ManifestAgent>,
    channels: Vec<String>,
    supervision: ManifestSupervision,
    project_id: Option<String>,
    graph_id: Option<String>,
    broker_endpoint: Option<String>,
}
```

- [ ] **Step 3: Initialize in ManifestBuilder::new**

In `impl ManifestBuilder`, find `pub fn new() -> Self` (around line 147). It currently initializes 5 fields. Add the sixth:

```rust
pub fn new() -> Self {
    Self {
        agents: Vec::new(),
        channels: Vec::new(),
        supervision: ManifestSupervision {
            root_agent: String::from("swarm-supervisor"),
            default_policy: ManifestRestartPolicy::RestartOne,
            max_restarts: 5,
        },
        project_id: None,
        graph_id: None,
        broker_endpoint: None,
    }
}
```

- [ ] **Step 4: Add builder method**

Right after the existing `project_id` and `graph_id` builder methods (around line 162-170), add:

```rust
pub fn broker_endpoint(mut self, endpoint: String) -> Self {
    self.broker_endpoint = Some(endpoint);
    self
}
```

- [ ] **Step 5: Propagate field in build()**

Find the `pub fn build(self) -> SwarmManifest` method (search for `fn build`). It constructs a `SwarmManifest`. Wherever it does, add `broker_endpoint: self.broker_endpoint,` to the struct literal. If `build()` is missing or constructs through some other path, find the place where SwarmManifest is created from ManifestBuilder and add the field.

If you can't find a single `build()` — search for `SwarmManifest {` in the file. There should be exactly one place that constructs it. Add the field there.

- [ ] **Step 6: Verify compile**

```bash
cd "/Users/ganeshkhetawat/My OS/oxide-os" && cargo build 2>&1 | tail -10
```

Expected: builds clean.

- [ ] **Step 7: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add oxide-os/kernel/src/swarmscope/manifest.rs && git commit -m "feat(kernel): add broker_endpoint to SwarmManifest"
```

---

## Task 3: Add `broker_cap` and `broker_endpoint` fields to `Agent` struct

**Files:**
- Modify: `oxide-os/kernel/src/agent/registry.rs`

- [ ] **Step 1: Add imports + fields**

Open `oxide-os/kernel/src/agent/registry.rs`. At the top, the imports are currently:

```rust
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use super::{AgentId, AgentState, AgentConfig};
use crate::println;
```

Add this line after the `super::` import:

```rust
use crate::capability::CapId;
```

Then find the `Agent` struct (lines 8-16):

```rust
pub struct Agent {
    pub id: AgentId,
    pub config: AgentConfig,
    pub state: AgentState,
    pub parent: Option<AgentId>,
    pub children: Vec<AgentId>,
    pub restart_count: u32,
    pub max_restarts: u32,
}
```

Change it to add two new fields at the end:

```rust
pub struct Agent {
    pub id: AgentId,
    pub config: AgentConfig,
    pub state: AgentState,
    pub parent: Option<AgentId>,
    pub children: Vec<AgentId>,
    pub restart_count: u32,
    pub max_restarts: u32,
    pub broker_cap: Option<CapId>,
    pub broker_endpoint: String,
}
```

- [ ] **Step 2: Find and patch all Agent constructors**

Search the kernel for every place that constructs an `Agent`:

```bash
cd "/Users/ganeshkhetawat/My OS/oxide-os" && grep -rn "Agent {" --include="*.rs" kernel/src/agent/
```

Each constructor must be updated to include the new fields. Add them at the bottom of each struct literal:

```rust
broker_cap: None,
broker_endpoint: String::new(),
```

The loader will overwrite `broker_endpoint` to a real value via `get_mut` later. `None`/empty are safe defaults — agents that never call the broker (supervisor, reporter) keep these defaults.

- [ ] **Step 3: Verify compile**

```bash
cd "/Users/ganeshkhetawat/My OS/oxide-os" && cargo build 2>&1 | tail -20
```

Expected: builds clean. If you see "missing field `broker_cap` in initializer", you missed an Agent constructor — find it and patch.

- [ ] **Step 4: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add oxide-os/kernel/src/agent/registry.rs $(grep -rln "Agent {" oxide-os/kernel/src/agent/) && git commit -m "feat(kernel): add broker_cap + broker_endpoint to Agent struct"
```

---

## Task 4: Bump HTTP recv_deadline for Claude latency

**Files:**
- Modify: `oxide-os/kernel/src/net/http.rs:88` and `oxide-os/kernel/src/net/http.rs:191` (two occurrences)

- [ ] **Step 1: Locate both deadlines**

```bash
cd "/Users/ganeshkhetawat/My OS/oxide-os" && grep -n "recv_deadline" kernel/src/net/http.rs
```

You should see two lines, both with `let recv_deadline = crate::interrupts::ticks() + 1000;` (one in `get()`, one in `post()`).

- [ ] **Step 2: Change both to 5000**

Edit both occurrences from:

```rust
let recv_deadline = crate::interrupts::ticks() + 1000;
```

To:

```rust
let recv_deadline = crate::interrupts::ticks() + 5000;
```

Use `replace_all` if your editor supports it; otherwise change each individually. Both occurrences should now be `+ 5000`.

- [ ] **Step 3: Verify compile**

```bash
cd "/Users/ganeshkhetawat/My OS/oxide-os" && cargo build 2>&1 | tail -5
```

Expected: clean build.

- [ ] **Step 4: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add oxide-os/kernel/src/net/http.rs && git commit -m "fix(kernel): raise HTTP recv_deadline to 5s for Claude latency"
```

---

## Task 5: Add `auto_grant_broker_cap` helper in loader.rs

**Files:**
- Modify: `oxide-os/kernel/src/swarmscope/loader.rs`

- [ ] **Step 1: Add imports**

At the top of `loader.rs`, find the existing imports (lines 7-16). Add this line after the existing `crate::capability::` import:

```rust
use crate::capability::CapId;
```

Also add this import if not already present:

```rust
use super::broker;
```

- [ ] **Step 2: Add helper function**

After the existing `to_restart_policy` helper (around line 88), add this helper before `swarmscope_agent_entry`:

```rust
/// Auto-grant a network capability targeting the broker to the given agent.
/// Returns the CapId. Idempotent only in the sense that calling twice creates two caps.
fn auto_grant_broker_cap(agent_id: AgentId, endpoint: &str) -> Option<CapId> {
    let (host, port) = parse_endpoint(endpoint)?;
    let resource = ResourceRef::Network { host, port };
    let cap_id = CAP_TABLE
        .lock()
        .create_root(agent_id, resource, PermissionBits::CONNECT);
    Some(cap_id)
}

/// Parse "http://host:port" → ("host", port). Returns None on malformed input.
/// Ignores path component. Defaults port to 80 if absent.
fn parse_endpoint(endpoint: &str) -> Option<(String, u16)> {
    let stripped = endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))
        .unwrap_or(endpoint);
    let host_port = stripped.split('/').next()?;
    let (host, port) = match host_port.find(':') {
        Some(i) => {
            let port: u16 = host_port[i + 1..].parse().ok()?;
            (String::from(&host_port[..i]), port)
        }
        None => (String::from(host_port), 80u16),
    };
    Some((host, port))
}
```

- [ ] **Step 3: Verify compile**

```bash
cd "/Users/ganeshkhetawat/My OS/oxide-os" && cargo build 2>&1 | tail -10
```

Expected: clean build (warnings about unused functions are fine — they'll be wired in Task 7).

- [ ] **Step 4: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add oxide-os/kernel/src/swarmscope/loader.rs && git commit -m "feat(kernel): auto_grant_broker_cap helper + endpoint parser"
```

---

## Task 6: Add `setup_agent_channels` helper + per-agent channel cap grants

**Files:**
- Modify: `oxide-os/kernel/src/swarmscope/loader.rs`

- [ ] **Step 1: Add helper**

After the `auto_grant_broker_cap` helper from Task 5, add:

```rust
/// For each non-aggregator agent, create channel `agent.<name>.out`,
/// grant the agent PUBLISH on it, and grant the aggregator SUBSCRIBE on it.
///
/// Returns the CapId map (per agent name) so callers can store the publish cap
/// on the Agent record for later use by the entry function.
fn setup_agent_channels(
    manifest: &SwarmManifest,
    agent_ids: &[(String, AgentId)],
) -> alloc::collections::BTreeMap<String, CapId> {
    use alloc::collections::BTreeMap;

    let aggregator_id = agent_ids
        .iter()
        .find(|(name, _)| {
            manifest
                .agents
                .iter()
                .any(|a| a.name == *name && a.role == AgentRole::Aggregator)
        })
        .map(|(_, id)| *id);

    let mut publish_caps: BTreeMap<String, CapId> = BTreeMap::new();

    for (agent_name, agent_id) in agent_ids {
        let agent_def = match manifest.agents.iter().find(|a| a.name == *agent_name) {
            Some(d) => d,
            None => continue,
        };
        if agent_def.role == AgentRole::Aggregator {
            continue;
        }

        let channel_name = format!("agent.{}.out", agent_name);
        crate::ipc::channel::create(channel_name.clone());

        let mut table = CAP_TABLE.lock();
        let pub_cap = table.create_root(
            *agent_id,
            ResourceRef::Channel { name: channel_name.clone() },
            PermissionBits::PUBLISH,
        );
        publish_caps.insert(agent_name.clone(), pub_cap);

        if let Some(agg_id) = aggregator_id {
            table.create_root(
                agg_id,
                ResourceRef::Channel { name: channel_name },
                PermissionBits::SUBSCRIBE,
            );
        }
    }

    publish_caps
}
```

- [ ] **Step 2: Verify compile**

```bash
cd "/Users/ganeshkhetawat/My OS/oxide-os" && cargo build 2>&1 | tail -10
```

Expected: clean. Unused-function warning is fine.

- [ ] **Step 3: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add oxide-os/kernel/src/swarmscope/loader.rs && git commit -m "feat(kernel): setup_agent_channels helper grants publish/subscribe caps"
```

---

## Task 7: Wire helpers into `spawn_swarm` and store broker state on Agent

**Files:**
- Modify: `oxide-os/kernel/src/swarmscope/loader.rs:237-353` (the `spawn_swarm` function)

- [ ] **Step 1: Pick the broker endpoint at the top of spawn_swarm**

Inside `spawn_swarm`, right after the manifest banner print block (after the `println!("  ╚══...`) line), add:

```rust
let broker_endpoint: String = manifest
    .broker_endpoint
    .clone()
    .unwrap_or_else(|| String::from(broker::DEFAULT_BROKER_ENDPOINT));
println!("[swarmscope] broker endpoint: {}", broker_endpoint);
```

- [ ] **Step 2: After all agents are spawned, wire channels and broker caps**

The current `spawn_swarm` already populates `agent_ids: Vec<(String, AgentId)>`. After the second `for (idx, agent_def) in manifest.agents.iter().enumerate()` loop (around line 303, just before `// 5. Print the supervision tree`), insert:

```rust
// 4b. Auto-grant broker network cap to each individual/group_rep agent and
//     persist endpoint + cap on the registry entry.
for (agent_name, agent_id) in &agent_ids {
    let agent_def = match manifest.agents.iter().find(|a| a.name == *agent_name) {
        Some(d) => d,
        None => continue,
    };
    if matches!(agent_def.role, AgentRole::Individual | AgentRole::GroupRepresentative) {
        let cap = auto_grant_broker_cap(*agent_id, &broker_endpoint);
        let mut reg = agent::registry::REGISTRY.lock();
        if let Some(a) = reg.get_mut(*agent_id) {
            a.broker_cap = cap;
            a.broker_endpoint = broker_endpoint.clone();
        }
    }
}

// 4c. Setup per-agent pub/sub channels (agent.<name>.out) and grant caps.
let publish_caps = setup_agent_channels(manifest, &agent_ids);
// Store the publish cap on each agent so the entry function can use it.
// We piggyback on broker_cap-style storage but use a separate field path.
// For now, the agent entry can fetch the publish cap by name from its registry
// lookup since we also need a place to store it — add via separate Agent field.
```

(The third comment block is a hand-off: the publish cap needs to live somewhere the entry function can find it. We add a third field to `Agent` in the next step of THIS task — it's a logical pair with the channel topology so it stays in Task 7.)

- [ ] **Step 3: Add `publish_cap: Option<CapId>` to Agent**

Open `oxide-os/kernel/src/agent/registry.rs` and add one more field to `Agent`:

```rust
pub struct Agent {
    pub id: AgentId,
    pub config: AgentConfig,
    pub state: AgentState,
    pub parent: Option<AgentId>,
    pub children: Vec<AgentId>,
    pub restart_count: u32,
    pub max_restarts: u32,
    pub broker_cap: Option<CapId>,
    pub broker_endpoint: String,
    pub publish_cap: Option<CapId>,
}
```

Also update every `Agent { ... }` constructor (use the same grep + patch approach from Task 3 Step 2) to add `publish_cap: None,`.

- [ ] **Step 4: Wire publish_cap from publish_caps map into the registry**

Back in `spawn_swarm`, immediately after the `let publish_caps = setup_agent_channels(...);` line, add:

```rust
for (agent_name, cap_id) in &publish_caps {
    if let Some((_, agent_id)) = agent_ids.iter().find(|(n, _)| n == agent_name) {
        let mut reg = agent::registry::REGISTRY.lock();
        if let Some(a) = reg.get_mut(*agent_id) {
            a.publish_cap = Some(*cap_id);
        }
    }
}
```

- [ ] **Step 5: Verify compile**

```bash
cd "/Users/ganeshkhetawat/My OS/oxide-os" && cargo build 2>&1 | tail -15
```

Expected: clean build. If "missing field publish_cap" — find the Agent constructor you missed and patch.

- [ ] **Step 6: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add oxide-os/kernel/src/swarmscope/loader.rs oxide-os/kernel/src/agent/registry.rs && git commit -m "feat(kernel): wire broker caps + channel topology into spawn_swarm"
```

---

## Task 8: Rewrite `swarmscope_agent_entry` to POST + publish

**Files:**
- Modify: `oxide-os/kernel/src/swarmscope/loader.rs:95-150` (the `swarmscope_agent_entry` function)

- [ ] **Step 1: Replace the entire function**

Find `fn swarmscope_agent_entry() -> !` (around line 95). Delete the entire function body and replace with:

```rust
/// Generic agent entry point for SwarmScope simulation agents.
///
/// Each round: look up own name + caps + broker endpoint, POST to broker,
/// publish the parsed reply onto agent.<name>.out for the aggregator.
fn swarmscope_agent_entry() -> ! {
    use crate::interrupts;

    let my_id = scheduler::SCHEDULER.lock().current_id() as u64;

    // Brief stagger so agents don't all hit the broker at the same tick
    let stagger = (my_id as u64 % 8) * 50;
    let target = interrupts::ticks() + stagger;
    while interrupts::ticks() < target {
        if scheduler::should_reschedule() {
            scheduler::yield_now();
        }
    }

    // Snapshot what we need from the registry under one lock
    let (name, broker_cap, broker_endpoint, publish_cap) = {
        let reg = agent::registry::REGISTRY.lock();
        match reg.get(my_id) {
            Some(a) => (
                a.config.name.clone(),
                a.broker_cap,
                a.broker_endpoint.clone(),
                a.publish_cap,
            ),
            None => {
                println!("[swarm-agent:{}] not in registry — exiting", my_id);
                loop { scheduler::yield_now(); }
            }
        }
    };

    let broker_cap = match broker_cap {
        Some(c) => c,
        None => {
            println!("[swarm-agent:{}] no broker cap — idling", name);
            loop { scheduler::yield_now(); }
        }
    };

    let publish_channel = format!("agent.{}.out", name);
    println!(
        "[swarm-agent:{}] online — broker={} channel={}",
        name, broker_endpoint, publish_channel
    );

    let mut round = 0u32;
    let mut consecutive_failures = 0u32;
    let mut backoff_until: u64 = 0;

    loop {
        round += 1;

        if interrupts::ticks() < backoff_until {
            let target = interrupts::ticks() + 100;
            while interrupts::ticks() < target {
                if scheduler::should_reschedule() {
                    scheduler::yield_now();
                }
            }
            continue;
        }

        let prompt = format!("Round {}. What do you observe?", round);
        let start = interrupts::ticks();

        match broker::post_infer(&broker_endpoint, &name, &prompt, my_id, broker_cap) {
            Ok(reply) => {
                consecutive_failures = 0;
                let elapsed = interrupts::ticks().saturating_sub(start);
                let payload = format!(
                    "{{\"agent\":\"{}\",\"round\":{},\"text\":\"{}\",\"tokens_in\":{},\"tokens_out\":{},\"elapsed_ticks\":{}}}",
                    name,
                    round,
                    escape_for_log(&reply.text),
                    reply.tokens_in,
                    reply.tokens_out,
                    elapsed
                ).into_bytes();

                if let Some(pcap) = publish_cap {
                    if let Err(e) = crate::ipc::channel::publish(&publish_channel, my_id, payload, pcap) {
                        println!("[swarm-agent:{}] publish failed: {:?}", name, e);
                    }
                }

                if round % 5 == 0 {
                    println!(
                        "[swarm-agent:{}] round={} tokens_in={} tokens_out={} elapsed_ticks={}",
                        name, round, reply.tokens_in, reply.tokens_out, elapsed
                    );
                }
            }
            Err(e) => {
                println!("[swarm-agent:{}] infer failed: {:?}", name, e);
                consecutive_failures += 1;
                if consecutive_failures >= 3 {
                    backoff_until = interrupts::ticks() + 10000;
                    println!("[swarm-agent:{}] entering slow-poll (10s)", name);
                }
            }
        }

        // Sleep ~2s between rounds
        let target = interrupts::ticks() + 2000;
        while interrupts::ticks() < target {
            if scheduler::should_reschedule() {
                scheduler::yield_now();
            }
        }
    }
}

/// Replace control chars and quotes for safe embedding in a log payload.
fn escape_for_log(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push(' '),
            '\r' => {}
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out
}
```

- [ ] **Step 2: Verify compile**

```bash
cd "/Users/ganeshkhetawat/My OS/oxide-os" && cargo build 2>&1 | tail -20
```

Expected: clean build. The old simulation code (the `// In the full implementation this would go through pub/sub channels` block) is gone.

- [ ] **Step 3: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add oxide-os/kernel/src/swarmscope/loader.rs && git commit -m "feat(kernel): swarmscope_agent_entry POSTs to broker and publishes reply"
```

---

## Task 9: Tidy aggregator entry to log the new payload shape

**Files:**
- Modify: `oxide-os/kernel/src/swarmscope/loader.rs:179-201` (the `swarmscope_aggregator_entry` function)

The aggregator already drains its mailbox via `message::receive(my_id)`. Because `ipc::channel::publish` pushes into each subscriber's mailbox, no API change is needed — we just need to log the new JSON payload more usefully.

- [ ] **Step 1: Replace the aggregator function body**

Find `fn swarmscope_aggregator_entry() -> !` (around line 179). Replace the function with:

```rust
/// Aggregator entry point — collects findings from all agent.<name>.out channels.
/// Channel publishes are delivered into our mailbox (one Message per publish),
/// so we drain via the existing message::receive() API.
fn swarmscope_aggregator_entry() -> ! {
    use crate::interrupts;

    let my_id = scheduler::SCHEDULER.lock().current_id() as u64;
    println!("[swarm-aggregator] online — draining inbox for agent.*.out publishes");
    let mut total = 0u32;

    loop {
        while let Some(msg) = message::receive(my_id) {
            total += 1;
            if let Ok(s) = core::str::from_utf8(&msg.payload) {
                // Truncate very long lines to keep serial output readable
                let preview: String = s.chars().take(220).collect();
                println!("[swarm-aggregator] #{} {}", total, preview);
            } else {
                println!("[swarm-aggregator] #{} ({} bytes, non-utf8)", total, msg.payload.len());
            }
        }

        if scheduler::should_reschedule() {
            scheduler::yield_now();
        }
    }
}
```

- [ ] **Step 2: Verify compile**

```bash
cd "/Users/ganeshkhetawat/My OS/oxide-os" && cargo build 2>&1 | tail -10
```

Expected: clean build.

- [ ] **Step 3: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add oxide-os/kernel/src/swarmscope/loader.rs && git commit -m "feat(kernel): aggregator logs new agent.*.out payload shape"
```

---

## Task 10: End-to-end integration test (manual, in QEMU)

**No code changes** — boot the kernel against the running broker and verify the expected serial output. This is the only meaningful "test" for a no_std kernel.

- [ ] **Step 1: Start the broker on the host**

In one terminal:

```bash
cd "/Users/ganeshkhetawat/My OS/console" && \
  ANTHROPIC_API_KEY="$ANTHROPIC_API_KEY" \
  BROKER_MANIFEST=server/broker/examples/manifest-2agents.json \
  npm run broker
```

Expected output:
```
[broker] listening on http://127.0.0.1:3101
[broker] provisioned agents (2): alice, bob
```

Leave it running. If you don't have `ANTHROPIC_API_KEY` set, the broker will exit — paste your key into the shell first.

- [ ] **Step 2: Build the kernel ISO**

In a second terminal:

```bash
cd "/Users/ganeshkhetawat/My OS/oxide-os" && make iso 2>&1 | tail -5
```

Expected: ISO built at `oxide-os/oxide-os.iso`. Build may take 30-60s on first run.

- [ ] **Step 3: Verify the boot manifest has matching agent names**

The kernel boots with a hardcoded test manifest. Open `oxide-os/kernel/src/main.rs` and search for the manifest construction (it uses `ManifestBuilder`). Confirm the agents in it match the broker's manifest names. The broker fixture defines `alice` and `bob`; the kernel's test manifest may use different names.

If the kernel manifest uses different names, you have two choices:
- (a) Add an alice + bob agent to the kernel's test manifest with `role: Individual` and `model: Remote { endpoint: "..." }`. Cleaner.
- (b) Run the broker with a manifest whose agent names match the kernel's existing test manifest. Faster.

For first verification, do (b): copy the kernel's agent names into a new broker manifest file:

```bash
cp "/Users/ganeshkhetawat/My OS/console/server/broker/examples/manifest-2agents.json" \
   "/Users/ganeshkhetawat/My OS/console/server/broker/examples/manifest-kernel.json"
```

Then edit `manifest-kernel.json` to use the same agent names as `oxide-os/kernel/src/main.rs`. Restart the broker pointing at the new manifest.

- [ ] **Step 4: Boot the kernel**

```bash
cd "/Users/ganeshkhetawat/My OS/oxide-os" && make run-headless 2>&1 | tee /tmp/oxide-boot.log
```

Let it run for ~30 seconds, then Ctrl+C.

- [ ] **Step 5: Verify expected output in serial log**

Check `/tmp/oxide-boot.log` for these lines (agent names may differ — substitute yours):

```bash
grep -E "swarmscope.*broker|swarm-agent.*online|swarm-agent.*round=|swarm-aggregator.*#|http.*POST.*3101" /tmp/oxide-boot.log | head -20
```

Expected to see:
- `[swarmscope] broker endpoint: http://10.0.2.2:3101`
- `[swarm-agent:<name>] online — broker=http://10.0.2.2:3101 channel=agent.<name>.out`
- `[http] POST 10.0.2.2:3101/v1/agent/<name>/infer (N bytes, task X)` (one per round)
- `[swarm-agent:<name>] round=5 tokens_in=NN tokens_out=NN elapsed_ticks=NN` (every 5 rounds)
- `[swarm-aggregator] #N {"agent":"<name>","round":1,"text":"...","tokens_in":NN,...}`

- [ ] **Step 6: Verify broker received the requests**

Check the broker terminal — it should be logging incoming POSTs (Anthropic SDK logs by default include request IDs, or you can add console.log in `routes.js` for debug).

- [ ] **Step 7: Test failure recovery**

Stop the broker (Ctrl+C in its terminal). Watch the kernel log — within ~5s you should see:

- `[swarm-agent:<name>] infer failed: Http(ConnectionFailed)` (or similar)
- After 3 failures: `[swarm-agent:<name>] entering slow-poll (10s)`

Restart the broker. Within ~10s the kernel should resume successful inferences.

- [ ] **Step 8: Document the result**

Append a short note to `docs/superpowers/specs/2026-05-31-kernel-broker-wiring-design.md` under a new "Verification log" section, recording: date, kernel boot count, observed message rate, any anomalies.

- [ ] **Step 9: Final commit (if you edited the spec or test manifest)**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add -A docs/ console/server/broker/examples/ && git commit -m "docs(kernel): verification log for kernel↔broker wiring"
```

(Only run this if there ARE changes — `git status` first.)

---

## Done criteria

- Kernel compiles clean (`cargo build` from `oxide-os/` exits 0).
- Booting in QEMU with broker running produces real Claude responses in the serial log under `[swarm-aggregator]`.
- Stopping the broker triggers the 3-failure → slow-poll path; restarting recovers.
- All 10 tasks committed individually on `feat/kernel-broker-wiring` branch.

## Out of scope (followups)

- TLS in kernel (still talks plaintext to broker; broker handles HTTPS to Claude).
- Ring-3 user space.
- Streaming responses.
- Per-agent prompt scheduling driven by the supervisor (currently each agent self-paces ~2s per round).
- Persistent history across kernel reboots.
- Bridging aggregator output into the console WebSocket dashboard so live agent thoughts appear in the browser.
