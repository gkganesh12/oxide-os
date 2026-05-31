# Oxide Host Broker

HTTP service on `:3101` that turns Oxide kernel agents into real Claude agents.

The kernel boots in QEMU and runs supervised agents defined by a SwarmScope manifest. Each kernel agent POSTs prompts to this broker over virtio-net; the broker maintains a per-agent Claude session (with prompt caching on the system prompt) and routes the reply back.

## Run

```bash
export ANTHROPIC_API_KEY=sk-ant-...
export BROKER_MANIFEST=server/broker/examples/manifest-2agents.json
npm run broker
```

## Endpoints

- `GET  /v1/health` → `{ status: "ok", agent_count }`
- `GET  /v1/agents` → `{ agents: [{ name, role, model, history_length }] }`
- `POST /v1/agent/:name/infer` — body `{ message }` → `{ agent, text, usage, stop_reason }`

## Test

```bash
npm run broker:test                          # stubbed SDK, no API calls (17 tests)
ANTHROPIC_API_KEY=sk-... npm run broker:test # also runs the live test (18 tests)
```

## Environment

| Var | Default | Purpose |
|---|---|---|
| `ANTHROPIC_API_KEY` | required | Claude API key |
| `BROKER_PORT` | `3101` | HTTP port |
| `BROKER_MANIFEST` | repo schema path | SwarmScope bridge manifest |

## Module layout

| File | Responsibility |
|---|---|
| `index.js` | Bootstrap — wires env → manifest → registry → routes → server |
| `manifest.js` | Pure JSON loader + required-field validation |
| `session.js` | `AgentSession` — wraps Anthropic SDK, holds per-agent history, sends cached system prompt |
| `registry.js` | `AgentRegistry` — `Map<name, AgentSession>`, built from manifest |
| `routes.js` | HTTP request dispatcher; takes a registry by injection |
| `examples/manifest-2agents.json` | Minimal 2-agent fixture used by tests and demos |
