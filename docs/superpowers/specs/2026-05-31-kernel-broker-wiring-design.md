# Kernel → Host Broker Wiring Design

> **Status:** Approved 2026-05-31. Spec for the second half of the host-broker work — wiring Oxide kernel agents to actually call the broker (PR #13) instead of looping with fake JSON.

## Goal

Replace the simulated agent loop in `oxide-os/kernel/src/swarmscope/loader.rs` so that supervised kernel agents send real Claude inference requests through the host broker on `:3101` over virtio-net, publish each reply onto a per-agent pub/sub channel, and let the aggregator collect all of them.

This is the "production-shaped" cut: real network capabilities, explicit channel topology, deterministic error handling.

## Non-goals

- TLS in the kernel (broker is plaintext over QEMU SLIRP — TLS is a separate roadmap item)
- Ring-3 user space (agents still run in-kernel; ring-3 is its own multi-week project)
- Persistent agent history (each kernel boot starts fresh sessions on the broker)
- Streaming responses (one-shot request/response; streaming is a future optimization)
- Kernel-side unit tests (kernel tests need ring-3 — verification is via QEMU + live broker)

## Architecture

```
┌──────────────────────────────────────────────────────┐
│ Oxide OS Kernel (QEMU)                               │
│                                                      │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐    │
│  │ agent_entry│  │ agent_entry│  │ aggregator │    │
│  │  (alice)   │  │   (bob)    │  │            │    │
│  │            │  │            │  │            │    │
│  │ 1. lookup  │  │ 1. lookup  │  │ sub:       │    │
│  │ 2. POST →  │  │ 2. POST →  │  │  agent.*.out│    │
│  │ 3. publish │  │ 3. publish │  │            │    │
│  │ 4. sleep   │  │ 4. sleep   │  │ loop:      │    │
│  └─────┬──────┘  └─────┬──────┘  │  drain     │    │
│        │ pub:          │ pub:    │  log       │    │
│        │ agent.alice.out│ agent.bob.out───────┘   │
│        ▼               ▼                            │
│  ┌──────────────────────────────────────────────┐  │
│  │ ipc::channel (pub/sub)                       │  │
│  └──────────────────────────────────────────────┘  │
│                                                     │
│            │ net::http::post(...)                  │
│            │   url = http://10.0.2.2:3101/v1/      │
│            │   body = {"message":"..."}            │
│            ▼                                        │
│  ┌──────────────────────────────────────────────┐  │
│  │ smoltcp + virtio-net + firewall + cap-check  │  │
│  └──────────────────────────────────────────────┘  │
└────────────────────────┼────────────────────────────┘
                         │ (QEMU SLIRP: 10.0.2.2 = host)
                         ▼
                ┌─────────────────────┐
                │ Host broker :3101   │ ──HTTPS──▶ Claude API
                │ (from PR #13)       │
                └─────────────────────┘
```

## Data flow (single round)

1. **Lookup** — `swarmscope_agent_entry()` reads `task_id = scheduler.current_id()`, fetches `Agent { name, broker_cap, .. }` from `agent::registry::REGISTRY`.
2. **Build request** — URL = `format!("{}/v1/agent/{}/infer", broker_endpoint, name)`. Body = JSON `{"message":"<prompt-for-this-round>"}`.
3. **POST** — `net::http::post(url, body, task_id, broker_cap)` returns `Result<HttpResponse, HttpError>`.
4. **Parse** — `HttpResponse.body` is JSON; extract `text`, `usage.input_tokens`, `usage.output_tokens`. (Minimal `find`-based parser; no serde dep in this hot path — alternatively use the existing `serde_json_core` if already in the kernel dependency tree.)
5. **Publish** — `ipc::channel::publish("agent.<name>.out", payload)` where payload = JSON `{"agent","round","text","tokens_in","tokens_out","elapsed_ms"}`.
6. **Sleep** — `sleep_ticks(round_delay)`, loop.

## Components

### `swarmscope/broker.rs` (new module)

Pure helper. No global state. No async.

```rust
pub const DEFAULT_BROKER_ENDPOINT: &str = "http://10.0.2.2:3101";

#[derive(Debug)]
pub enum BrokerError {
    Network(crate::net::http::HttpError),
    Non2xx(u16),
    ParseFailure,
    Unreachable,
}

pub struct InferReply {
    pub text: String,
    pub tokens_in: u32,
    pub tokens_out: u32,
}

pub fn post_infer(
    endpoint: &str,
    agent_name: &str,
    user_message: &str,
    task_id: TaskId,
    net_cap: CapId,
) -> Result<InferReply, BrokerError>;
```

Internals:
- Build URL `{endpoint}/v1/agent/{agent_name}/infer`.
- Build body `{"message": "<escaped>"}` — minimal JSON escaping (replace `"` and `\` and control chars).
- Call `net::http::post()`.
- On non-2xx: return `BrokerError::Non2xx(status)`.
- On 2xx: extract `text`, `usage.input_tokens`, `usage.output_tokens` from response body. If parse fails: `ParseFailure`.

### `swarmscope/loader.rs` (rewritten entry points)

**New helper:** `auto_grant_broker_cap(agent_id, endpoint) -> Option<CapId>` — called inside `spawn_swarm` for every individual/group_rep agent. Parses host:port from endpoint, creates a root cap on `ResourceRef::Network { host, port }` with `CONNECT`, returns the `CapId`, stores it on the `Agent` record in the registry.

**New helper:** `setup_agent_channels(manifest)` — called inside `spawn_swarm` before agent spawn. For each non-aggregator agent, calls `channel::create("agent.<name>.out")`. Grants the agent `PUBLISH` permission and the aggregator `SUBSCRIBE` permission on its channel.

**Rewritten:** `swarmscope_agent_entry()`:

```rust
fn swarmscope_agent_entry() -> ! {
    let my_id = scheduler::SCHEDULER.lock().current_id() as u64;
    let (name, broker_cap, endpoint) = {
        let reg = agent::registry::REGISTRY.lock();
        let agent = reg.get(my_id).expect("agent not in registry");
        (agent.name.clone(), agent.broker_cap, agent.broker_endpoint.clone())
    };
    let broker_cap = broker_cap.expect("agent has no broker cap");

    println!("[swarm-agent:{}] online — using broker {}", name, endpoint);
    let mut round = 0u32;
    let mut consecutive_failures = 0u32;
    let mut backoff_until: u64 = 0;

    loop {
        round += 1;
        if interrupts::ticks() < backoff_until {
            sleep_ticks(100);
            continue;
        }

        let prompt = build_round_prompt(&name, round);  // round-specific prompt
        let start = interrupts::ticks();
        match broker::post_infer(&endpoint, &name, &prompt, my_id, broker_cap) {
            Ok(reply) => {
                consecutive_failures = 0;
                let elapsed_ms = interrupts::ticks().saturating_sub(start);
                let payload = build_aggregator_payload(&name, round, &reply, elapsed_ms);
                let _ = channel::publish(&format!("agent.{}.out", name), payload);
            }
            Err(e) => {
                println!("[swarm-agent:{}] infer failed: {:?}", name, e);
                consecutive_failures += 1;
                if consecutive_failures >= 3 {
                    backoff_until = interrupts::ticks() + 10000;
                    println!("[swarm-agent:{}] entering slow-poll mode", name);
                }
            }
        }

        sleep_ticks(2000);  // ~2s between rounds
    }
}
```

**Rewritten:** `swarmscope_aggregator_entry()`:

```rust
fn swarmscope_aggregator_entry() -> ! {
    let my_id = scheduler::SCHEDULER.lock().current_id() as u64;
    let channels: Vec<String> = {
        let reg = agent::registry::REGISTRY.lock();
        reg.all_ids().iter()
            .filter_map(|id| reg.get(*id))
            .filter(|a| a.role != AgentRole::Aggregator)
            .map(|a| format!("agent.{}.out", a.name))
            .collect()
    };
    for ch in &channels {
        let _ = channel::subscribe(ch, my_id);
    }
    println!("[swarm-aggregator] subscribed to {} channels", channels.len());

    let mut total = 0u32;
    loop {
        for ch in &channels {
            while let Some(msg) = channel::next_message(ch, my_id) {
                total += 1;
                let preview = msg.payload.iter().take(80).copied().collect::<Vec<u8>>();
                if let Ok(s) = core::str::from_utf8(&preview) {
                    println!("[swarm-aggregator] msg #{}: {}", total, s);
                }
            }
        }
        sleep_ticks(50);
    }
}
```

(Method names like `channel::next_message` / `channel::subscribe` may not match the existing API exactly — those are to be reconciled against `ipc::channel` during implementation.)

### `swarmscope/manifest.rs`

Add one optional field to `SwarmManifest`:

```rust
pub broker_endpoint: Option<String>,
```

When `None`, the loader uses `broker::DEFAULT_BROKER_ENDPOINT`.

### `agent/registry.rs`

Add two fields to `Agent`:

```rust
pub broker_cap: Option<CapId>,
pub broker_endpoint: String,
```

Both populated by the loader at spawn time.

## Capability model

| Agent role | Auto-granted caps | Manifest override |
|---|---|---|
| Individual / GroupRep | `Network { 10.0.2.2, 3101 }` with `CONNECT` | Yes — if manifest lists a Network resource, use it instead |
| Aggregator | `Channel { agent.<name>.out }` with `SUBSCRIBE` for every other agent | No (always auto) |
| Supervisor | None (unchanged) | — |
| Reporter | None (unchanged) | — |

## Error handling (deterministic policy)

| Failure | Action |
|---|---|
| `HttpError::ConnectionFailed` | Log, increment failure counter, retry next round |
| `HttpError::ReceiveFailed` (timeout 5s) | Log, increment failure counter, retry next round |
| `HttpError::CapabilityDenied` | Log + panic-print agent state, enter `loop { yield_now() }` |
| `BrokerError::Non2xx(status)` | Log `[swarm-agent:<name>] broker returned <status>`, increment counter, retry next round |
| `BrokerError::ParseFailure` | Log first 200 bytes of response, increment counter, retry next round |
| 3+ consecutive failures | Enter slow-poll: one attempt per 10000 ticks (~10s) until success, then reset counter |

## Boot configuration

- Default endpoint: `BROKER_ENDPOINT = "http://10.0.2.2:3101"` (QEMU SLIRP gateway).
- Override: `SwarmManifest.broker_endpoint = Some("http://...")`. Wired through to each agent at spawn time.
- No new boot args. Future work can wire a kernel cmdline `broker_endpoint=` parameter through `main.rs`.

## Test plan

Kernel unit tests are out of scope (need ring-3). Verification is end-to-end:

1. Build kernel: `cd oxide-os && ./scripts/build-iso.sh` (or whatever the current build incantation is).
2. Start host broker: `BROKER_MANIFEST=server/broker/examples/manifest-2agents.json npm run broker`.
3. Boot kernel: `npm run bridge` (the existing console QEMU bridge) — kernel comes up, loads test manifest (alice + bob), spawns agents.
4. **Expected serial output**:
   - `[swarm-agent:alice] online — using broker http://10.0.2.2:3101`
   - `[swarm-agent:bob] online — using broker http://10.0.2.2:3101`
   - `[http] POST 10.0.2.2:3101/v1/agent/alice/infer (N bytes, task X)`
   - `[swarm-aggregator] subscribed to 2 channels`
   - `[swarm-aggregator] msg #1: {"agent":"alice","round":1,"text":"...","tokens_in":...,"tokens_out":...,"elapsed_ms":...}`
5. **Expected broker logs**:
   - `POST /v1/agent/alice/infer` and `POST /v1/agent/bob/infer` arriving in parallel
   - Non-zero `output_tokens` on responses
6. **Failure scenarios verified manually**:
   - Stop broker → kernel should log `infer failed` 3× then enter slow-poll
   - Restart broker → kernel should recover within ~10 ticks (~10s)

## Risks & open questions

1. **smoltcp + QEMU SLIRP TCP behavior** — the existing `net::http::post` uses 1000-tick recv deadline; Claude responses can take 1-5s. Need to raise deadline to 5000+. May need stack polling adjustments.
2. **Kernel heap pressure** — each Claude response is up to 1024 tokens (~4KB text + JSON overhead). With multiple agents each holding response buffers, heap usage grows. Current heap is fixed 1 MiB per TODO.md. Probably fine for 2-5 agents; will hit limits at scale.
3. **JSON parsing** — kernel currently has no JSON dep (loader hand-builds the simulated payload). For parsing the broker reply, either: (a) add `serde_json_core` (no_std-friendly), or (b) hand-roll a `find`-based extractor for just the `text` and `usage` fields. Recommendation: (b) for first cut; revisit if more agents need rich parsing.
4. **No retry with exponential backoff** — slow-poll is a fixed 10s. Acceptable for a demo; would tune for production.
5. **Channel API mismatch** — design assumes `channel::publish`, `channel::subscribe`, `channel::next_message`. Real names in `ipc::channel` need verification during implementation; the design intent (pub/sub fan-out from N producers to 1 aggregator consumer) is what matters.

## File summary

| File | Status | LOC delta (approx) |
|---|---|---|
| `oxide-os/kernel/src/swarmscope/broker.rs` | new | +120 |
| `oxide-os/kernel/src/swarmscope/loader.rs` | rewrite entry fns + helpers | +80 / -50 |
| `oxide-os/kernel/src/swarmscope/manifest.rs` | add field | +3 |
| `oxide-os/kernel/src/swarmscope/mod.rs` | export new module | +1 |
| `oxide-os/kernel/src/agent/registry.rs` | add 2 fields to Agent | +6 |

Total: roughly +210 / -50.
