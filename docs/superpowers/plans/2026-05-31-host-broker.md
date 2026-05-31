# Oxide Host Broker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Node.js host broker that reads `swarmscope_bridge_schema.json`, provisions one Claude session per agent, and exposes `POST /v1/agent/:name/infer` so Oxide kernel agents (running in QEMU) can swap their fake JSON loop for real Claude inference calls over virtio-net.

**Architecture:** Standalone HTTP service on port 3101, living alongside the existing `console/server/bridge.js` (port 3100). Uses Node's built-in `http` module (no Express) and `@anthropic-ai/sdk` with prompt caching enabled per agent system prompt. Each agent gets an `AgentSession` holding its own conversation history. Stateful in-memory only — restart broker = fresh sessions. Kernel-side changes are out of scope for this plan (follow-up plan).

**Tech Stack:** Node 24+ (existing `console/` uses Node 25), `@anthropic-ai/sdk` ^0.39, built-in `http`, built-in `node:test` for tests. No new framework deps.

---

## File Structure

```
console/
├── package.json                            # MODIFY: add @anthropic-ai/sdk + broker script
└── server/
    └── broker/
        ├── index.js                        # NEW: http server bootstrap on 3101
        ├── manifest.js                     # NEW: loadManifest(path)
        ├── session.js                      # NEW: AgentSession class
        ├── registry.js                     # NEW: AgentRegistry (Map<name, AgentSession>)
        ├── routes.js                       # NEW: route dispatcher (infer, list, health)
        ├── examples/
        │   └── manifest-2agents.json       # NEW: test manifest (tiny, 2 agents)
        └── test/
            ├── manifest.test.js            # NEW
            ├── session.test.js             # NEW (uses stub SDK)
            ├── registry.test.js            # NEW
            ├── routes.test.js              # NEW
            └── live.test.js                # NEW (gated by ANTHROPIC_API_KEY)
```

**Boundaries:**
- `manifest.js` — pure JSON loader + validator. No I/O beyond `fs.readFileSync`.
- `session.js` — wraps Anthropic SDK. Single agent, holds history. Takes SDK client by constructor injection (for test stubs).
- `registry.js` — maps agent names → sessions, built from manifest.
- `routes.js` — pure request dispatcher. Takes registry by injection.
- `index.js` — wires everything: load env, load manifest, build SDK client, build registry, start server.

---

## Task 1: Bootstrap dependencies and scaffolding

**Files:**
- Modify: `console/package.json`
- Create: `console/server/broker/` (directory)
- Create: `console/server/broker/test/` (directory)
- Create: `console/server/broker/examples/` (directory)

- [ ] **Step 1: Install Anthropic SDK**

```bash
cd "/Users/ganeshkhetawat/My OS/console" && npm install @anthropic-ai/sdk@^0.39.0
```

Expected: package added to `dependencies`, `package-lock.json` updated.

- [ ] **Step 2: Add broker scripts to package.json**

Edit `console/package.json` `scripts` block — add two lines after `"bridge"`:

```json
"broker": "node server/broker/index.js",
"broker:test": "node --test 'server/broker/test/**/*.test.js'"
```

- [ ] **Step 3: Create directories**

```bash
mkdir -p "/Users/ganeshkhetawat/My OS/console/server/broker/test" "/Users/ganeshkhetawat/My OS/console/server/broker/examples"
```

- [ ] **Step 4: Verify scaffolding**

```bash
cd "/Users/ganeshkhetawat/My OS/console" && npm run broker:test 2>&1 | head -5
```

Expected: `node --test` runs, finds 0 tests, exits 0 (or notes "no test files" — both are fine; we just want the script wired).

- [ ] **Step 5: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add console/package.json console/package-lock.json && git commit -m "feat(broker): scaffold host broker module with Anthropic SDK"
```

---

## Task 2: Create the test manifest fixture

**Files:**
- Create: `console/server/broker/examples/manifest-2agents.json`

- [ ] **Step 1: Write the fixture**

Create `console/server/broker/examples/manifest-2agents.json`:

```json
{
  "version": "1.0.0",
  "metadata": {
    "project_id": "broker-test",
    "agent_count": 2,
    "source": "swarmscope"
  },
  "agents": [
    {
      "name": "alice",
      "system_prompt": "You are Alice, a concise research assistant. Reply in one short sentence.",
      "role": "individual",
      "model": { "type": "remote", "model_id": "claude-haiku-4-5-20251001" },
      "capabilities": {
        "resources": [
          { "type": "channel", "name": "agent.alice.out", "permissions": ["publish"] }
        ]
      }
    },
    {
      "name": "bob",
      "system_prompt": "You are Bob, an aggregator. Reply with a single word summary.",
      "role": "aggregator",
      "model": { "type": "remote", "model_id": "claude-haiku-4-5-20251001" },
      "capabilities": {
        "resources": [
          { "type": "channel", "name": "agent.alice.out", "permissions": ["subscribe"] }
        ]
      }
    }
  ],
  "channels": ["agent.alice.out"],
  "supervision": {
    "root_agent": "bob",
    "default_policy": "restart_one",
    "max_restarts": 5
  }
}
```

- [ ] **Step 2: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add console/server/broker/examples/manifest-2agents.json && git commit -m "feat(broker): add 2-agent test manifest fixture"
```

---

## Task 3: Manifest loader (TDD)

**Files:**
- Create: `console/server/broker/manifest.js`
- Test: `console/server/broker/test/manifest.test.js`

- [ ] **Step 1: Write failing tests**

Create `console/server/broker/test/manifest.test.js`:

```javascript
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { writeFileSync, rmSync, mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { loadManifest } from '../manifest.js'

const __dirname = dirname(fileURLToPath(import.meta.url))
const FIXTURE = resolve(__dirname, '../examples/manifest-2agents.json')

test('loadManifest reads valid manifest and returns agents/channels/supervision', () => {
  const m = loadManifest(FIXTURE)
  assert.equal(m.agents.length, 2)
  assert.equal(m.agents[0].name, 'alice')
  assert.equal(m.agents[1].name, 'bob')
  assert.deepEqual(m.channels, ['agent.alice.out'])
  assert.equal(m.supervision.root_agent, 'bob')
})

test('loadManifest throws on missing system_prompt', () => {
  const dir = mkdtempSync(resolve(tmpdir(), 'broker-'))
  const bad = resolve(dir, 'bad.json')
  writeFileSync(bad, JSON.stringify({
    version: '1.0.0',
    agents: [{ name: 'x', role: 'individual', model: { type: 'remote' }, capabilities: {} }],
    channels: [],
    supervision: { root_agent: 'x', default_policy: 'restart_one' }
  }))
  assert.throws(() => loadManifest(bad), /system_prompt/)
  rmSync(dir, { recursive: true, force: true })
})

test('loadManifest throws on duplicate agent names', () => {
  const dir = mkdtempSync(resolve(tmpdir(), 'broker-'))
  const bad = resolve(dir, 'dup.json')
  writeFileSync(bad, JSON.stringify({
    version: '1.0.0',
    agents: [
      { name: 'x', system_prompt: 'p', role: 'individual', model: { type: 'remote' }, capabilities: {} },
      { name: 'x', system_prompt: 'p', role: 'individual', model: { type: 'remote' }, capabilities: {} }
    ],
    channels: [],
    supervision: { root_agent: 'x', default_policy: 'restart_one' }
  }))
  assert.throws(() => loadManifest(bad), /duplicate/i)
  rmSync(dir, { recursive: true, force: true })
})
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cd "/Users/ganeshkhetawat/My OS/console" && npm run broker:test 2>&1 | tail -20
```

Expected: 3 failures, all due to `Cannot find module '../manifest.js'`.

- [ ] **Step 3: Implement manifest.js**

Create `console/server/broker/manifest.js`:

```javascript
import { readFileSync } from 'node:fs'

const REQUIRED_AGENT_FIELDS = ['name', 'system_prompt', 'model', 'role']

export function loadManifest(path) {
  const raw = readFileSync(path, 'utf8')
  const m = JSON.parse(raw)

  if (!Array.isArray(m.agents) || m.agents.length === 0) {
    throw new Error('manifest: agents must be a non-empty array')
  }

  const seen = new Set()
  for (const agent of m.agents) {
    for (const field of REQUIRED_AGENT_FIELDS) {
      if (agent[field] === undefined || agent[field] === null) {
        throw new Error(`manifest: agent "${agent.name ?? '<unnamed>'}" missing required field: ${field}`)
      }
    }
    if (seen.has(agent.name)) {
      throw new Error(`manifest: duplicate agent name: ${agent.name}`)
    }
    seen.add(agent.name)
  }

  if (!m.supervision?.root_agent) {
    throw new Error('manifest: supervision.root_agent required')
  }

  return {
    version: m.version,
    metadata: m.metadata ?? {},
    agents: m.agents,
    channels: m.channels ?? [],
    supervision: m.supervision,
  }
}
```

- [ ] **Step 4: Run tests to confirm pass**

```bash
cd "/Users/ganeshkhetawat/My OS/console" && npm run broker:test 2>&1 | tail -10
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add console/server/broker/manifest.js console/server/broker/test/manifest.test.js && git commit -m "feat(broker): manifest loader with required-field validation"
```

---

## Task 4: AgentSession class (TDD with stub SDK)

**Files:**
- Create: `console/server/broker/session.js`
- Test: `console/server/broker/test/session.test.js`

- [ ] **Step 1: Write failing tests**

Create `console/server/broker/test/session.test.js`:

```javascript
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { AgentSession } from '../session.js'

function makeStubClient(scriptedResponses) {
  const calls = []
  return {
    calls,
    messages: {
      create: async (params) => {
        calls.push(params)
        const text = scriptedResponses.shift() ?? 'default reply'
        return {
          id: `msg_${calls.length}`,
          content: [{ type: 'text', text }],
          usage: { input_tokens: 10, output_tokens: 5, cache_read_input_tokens: 0 },
          stop_reason: 'end_turn',
        }
      },
    },
  }
}

test('AgentSession.infer calls SDK with system prompt + user message', async () => {
  const client = makeStubClient(['hi there'])
  const s = new AgentSession({
    name: 'alice',
    systemPrompt: 'You are Alice.',
    modelId: 'claude-haiku-4-5-20251001',
    client,
  })
  const out = await s.infer('hello')
  assert.equal(out.text, 'hi there')
  assert.equal(client.calls.length, 1)
  assert.equal(client.calls[0].model, 'claude-haiku-4-5-20251001')
  assert.equal(client.calls[0].messages[0].role, 'user')
  assert.equal(client.calls[0].messages[0].content, 'hello')
})

test('AgentSession sends system prompt with cache_control: ephemeral', async () => {
  const client = makeStubClient(['ok'])
  const s = new AgentSession({
    name: 'alice', systemPrompt: 'You are Alice.', modelId: 'claude-haiku-4-5-20251001', client,
  })
  await s.infer('hi')
  const system = client.calls[0].system
  assert.ok(Array.isArray(system), 'system should be an array block')
  assert.equal(system[0].type, 'text')
  assert.equal(system[0].text, 'You are Alice.')
  assert.deepEqual(system[0].cache_control, { type: 'ephemeral' })
})

test('AgentSession preserves history across calls', async () => {
  const client = makeStubClient(['first reply', 'second reply'])
  const s = new AgentSession({
    name: 'alice', systemPrompt: 'p', modelId: 'claude-haiku-4-5-20251001', client,
  })
  await s.infer('msg one')
  await s.infer('msg two')
  const secondCall = client.calls[1]
  assert.equal(secondCall.messages.length, 3)
  assert.equal(secondCall.messages[0].content, 'msg one')
  assert.equal(secondCall.messages[1].role, 'assistant')
  assert.equal(secondCall.messages[2].content, 'msg two')
})

test('AgentSession.infer returns usage stats', async () => {
  const client = makeStubClient(['ok'])
  const s = new AgentSession({
    name: 'alice', systemPrompt: 'p', modelId: 'claude-haiku-4-5-20251001', client,
  })
  const out = await s.infer('hi')
  assert.equal(out.usage.input_tokens, 10)
  assert.equal(out.usage.output_tokens, 5)
})
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cd "/Users/ganeshkhetawat/My OS/console" && npm run broker:test 2>&1 | tail -15
```

Expected: 4 new failures referencing `Cannot find module '../session.js'`.

- [ ] **Step 3: Implement session.js**

Create `console/server/broker/session.js`:

```javascript
const DEFAULT_MAX_TOKENS = 1024

export class AgentSession {
  constructor({ name, systemPrompt, modelId, client, maxTokens = DEFAULT_MAX_TOKENS }) {
    this.name = name
    this.systemPrompt = systemPrompt
    this.modelId = modelId
    this.client = client
    this.maxTokens = maxTokens
    this.history = []
  }

  async infer(userMessage) {
    this.history.push({ role: 'user', content: userMessage })

    const response = await this.client.messages.create({
      model: this.modelId,
      max_tokens: this.maxTokens,
      system: [{ type: 'text', text: this.systemPrompt, cache_control: { type: 'ephemeral' } }],
      messages: this.history,
    })

    const text = response.content
      .filter((block) => block.type === 'text')
      .map((block) => block.text)
      .join('')

    this.history.push({ role: 'assistant', content: text })

    return {
      text,
      usage: response.usage,
      stop_reason: response.stop_reason,
      id: response.id,
    }
  }

  reset() {
    this.history = []
  }
}
```

- [ ] **Step 4: Run tests to confirm pass**

```bash
cd "/Users/ganeshkhetawat/My OS/console" && npm run broker:test 2>&1 | tail -10
```

Expected: 7 tests pass (3 from Task 3 + 4 new).

- [ ] **Step 5: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add console/server/broker/session.js console/server/broker/test/session.test.js && git commit -m "feat(broker): AgentSession with cached system prompts and history"
```

---

## Task 5: AgentRegistry (TDD)

**Files:**
- Create: `console/server/broker/registry.js`
- Test: `console/server/broker/test/registry.test.js`

- [ ] **Step 1: Write failing tests**

Create `console/server/broker/test/registry.test.js`:

```javascript
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { loadManifest } from '../manifest.js'
import { AgentRegistry } from '../registry.js'
import { AgentSession } from '../session.js'

const __dirname = dirname(fileURLToPath(import.meta.url))
const FIXTURE = resolve(__dirname, '../examples/manifest-2agents.json')

function stubClient() {
  return { messages: { create: async () => ({ content: [{ type: 'text', text: '.' }], usage: {}, stop_reason: 'end_turn', id: 'x' }) } }
}

test('AgentRegistry.fromManifest provisions one session per agent', () => {
  const manifest = loadManifest(FIXTURE)
  const reg = AgentRegistry.fromManifest(manifest, stubClient())
  assert.equal(reg.size, 2)
  assert.ok(reg.get('alice') instanceof AgentSession)
  assert.ok(reg.get('bob') instanceof AgentSession)
})

test('AgentRegistry.get returns undefined for unknown agent', () => {
  const manifest = loadManifest(FIXTURE)
  const reg = AgentRegistry.fromManifest(manifest, stubClient())
  assert.equal(reg.get('nobody'), undefined)
})

test('AgentRegistry.list returns agent summaries', () => {
  const manifest = loadManifest(FIXTURE)
  const reg = AgentRegistry.fromManifest(manifest, stubClient())
  const list = reg.list()
  assert.equal(list.length, 2)
  assert.deepEqual(list[0], { name: 'alice', role: 'individual', model: 'claude-haiku-4-5-20251001', history_length: 0 })
})

test('AgentRegistry passes system prompt and model into each session', () => {
  const manifest = loadManifest(FIXTURE)
  const reg = AgentRegistry.fromManifest(manifest, stubClient())
  assert.match(reg.get('alice').systemPrompt, /Alice/)
  assert.equal(reg.get('bob').modelId, 'claude-haiku-4-5-20251001')
})
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cd "/Users/ganeshkhetawat/My OS/console" && npm run broker:test 2>&1 | tail -15
```

Expected: 4 new failures referencing `Cannot find module '../registry.js'`.

- [ ] **Step 3: Implement registry.js**

Create `console/server/broker/registry.js`:

```javascript
import { AgentSession } from './session.js'

const DEFAULT_MODEL = 'claude-haiku-4-5-20251001'

export class AgentRegistry {
  constructor() {
    this.sessions = new Map()
  }

  static fromManifest(manifest, client) {
    const reg = new AgentRegistry()
    for (const agent of manifest.agents) {
      const modelId = agent.model?.model_id ?? DEFAULT_MODEL
      reg.sessions.set(agent.name, new AgentSession({
        name: agent.name,
        systemPrompt: agent.system_prompt,
        modelId,
        client,
      }))
      reg.sessions.get(agent.name)._role = agent.role
    }
    return reg
  }

  get size() { return this.sessions.size }

  get(name) { return this.sessions.get(name) }

  list() {
    return Array.from(this.sessions.values()).map((s) => ({
      name: s.name,
      role: s._role,
      model: s.modelId,
      history_length: s.history.length,
    }))
  }
}
```

- [ ] **Step 4: Run tests to confirm pass**

```bash
cd "/Users/ganeshkhetawat/My OS/console" && npm run broker:test 2>&1 | tail -10
```

Expected: 11 tests pass.

- [ ] **Step 5: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add console/server/broker/registry.js console/server/broker/test/registry.test.js && git commit -m "feat(broker): AgentRegistry to provision sessions from manifest"
```

---

## Task 6: HTTP routes (TDD)

**Files:**
- Create: `console/server/broker/routes.js`
- Test: `console/server/broker/test/routes.test.js`

- [ ] **Step 1: Write failing tests**

Create `console/server/broker/test/routes.test.js`:

```javascript
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { loadManifest } from '../manifest.js'
import { AgentRegistry } from '../registry.js'
import { createHandler } from '../routes.js'

const __dirname = dirname(fileURLToPath(import.meta.url))
const FIXTURE = resolve(__dirname, '../examples/manifest-2agents.json')

function startServer(handler) {
  return new Promise((res) => {
    const srv = createServer(handler)
    srv.listen(0, '127.0.0.1', () => res(srv))
  })
}

function stubClient(replies) {
  let i = 0
  return {
    messages: {
      create: async () => ({
        content: [{ type: 'text', text: replies[i++] ?? 'stub' }],
        usage: { input_tokens: 1, output_tokens: 1 },
        stop_reason: 'end_turn',
        id: `msg_${i}`,
      }),
    },
  }
}

async function fetchJson(srv, path, init = {}) {
  const { port } = srv.address()
  const r = await fetch(`http://127.0.0.1:${port}${path}`, init)
  const body = r.headers.get('content-type')?.includes('json') ? await r.json() : await r.text()
  return { status: r.status, body }
}

test('GET /v1/health returns 200 with ok status', async () => {
  const reg = AgentRegistry.fromManifest(loadManifest(FIXTURE), stubClient([]))
  const srv = await startServer(createHandler(reg))
  const r = await fetchJson(srv, '/v1/health')
  assert.equal(r.status, 200)
  assert.equal(r.body.status, 'ok')
  assert.equal(r.body.agent_count, 2)
  srv.close()
})

test('GET /v1/agents returns agent summaries', async () => {
  const reg = AgentRegistry.fromManifest(loadManifest(FIXTURE), stubClient([]))
  const srv = await startServer(createHandler(reg))
  const r = await fetchJson(srv, '/v1/agents')
  assert.equal(r.status, 200)
  assert.equal(r.body.agents.length, 2)
  assert.equal(r.body.agents[0].name, 'alice')
  srv.close()
})

test('POST /v1/agent/:name/infer returns Claude reply', async () => {
  const reg = AgentRegistry.fromManifest(loadManifest(FIXTURE), stubClient(['hello world']))
  const srv = await startServer(createHandler(reg))
  const r = await fetchJson(srv, '/v1/agent/alice/infer', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ message: 'hi' }),
  })
  assert.equal(r.status, 200)
  assert.equal(r.body.text, 'hello world')
  assert.equal(r.body.agent, 'alice')
  srv.close()
})

test('POST /v1/agent/:name/infer returns 404 for unknown agent', async () => {
  const reg = AgentRegistry.fromManifest(loadManifest(FIXTURE), stubClient([]))
  const srv = await startServer(createHandler(reg))
  const r = await fetchJson(srv, '/v1/agent/nobody/infer', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ message: 'hi' }),
  })
  assert.equal(r.status, 404)
  srv.close()
})

test('POST /v1/agent/:name/infer returns 400 if message missing', async () => {
  const reg = AgentRegistry.fromManifest(loadManifest(FIXTURE), stubClient([]))
  const srv = await startServer(createHandler(reg))
  const r = await fetchJson(srv, '/v1/agent/alice/infer', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({}),
  })
  assert.equal(r.status, 400)
  srv.close()
})

test('concurrent inferences across different agents do not interfere', async () => {
  const reg = AgentRegistry.fromManifest(loadManifest(FIXTURE), stubClient(['A-reply', 'B-reply']))
  const srv = await startServer(createHandler(reg))
  const [a, b] = await Promise.all([
    fetchJson(srv, '/v1/agent/alice/infer', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ message: 'a' }),
    }),
    fetchJson(srv, '/v1/agent/bob/infer', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ message: 'b' }),
    }),
  ])
  assert.equal(a.status, 200)
  assert.equal(b.status, 200)
  assert.ok(['A-reply', 'B-reply'].includes(a.body.text))
  assert.ok(['A-reply', 'B-reply'].includes(b.body.text))
  assert.notEqual(a.body.text, b.body.text)
  srv.close()
})
```

- [ ] **Step 2: Run tests to confirm failure**

```bash
cd "/Users/ganeshkhetawat/My OS/console" && npm run broker:test 2>&1 | tail -15
```

Expected: 6 new failures referencing `Cannot find module '../routes.js'`.

- [ ] **Step 3: Implement routes.js**

Create `console/server/broker/routes.js`:

```javascript
async function readJsonBody(req) {
  const chunks = []
  for await (const chunk of req) chunks.push(chunk)
  const raw = Buffer.concat(chunks).toString('utf8')
  if (!raw) return {}
  return JSON.parse(raw)
}

function send(res, status, body) {
  res.writeHead(status, { 'content-type': 'application/json' })
  res.end(JSON.stringify(body))
}

export function createHandler(registry) {
  return async (req, res) => {
    try {
      const url = new URL(req.url, 'http://localhost')

      if (req.method === 'GET' && url.pathname === '/v1/health') {
        return send(res, 200, { status: 'ok', agent_count: registry.size })
      }

      if (req.method === 'GET' && url.pathname === '/v1/agents') {
        return send(res, 200, { agents: registry.list() })
      }

      const inferMatch = url.pathname.match(/^\/v1\/agent\/([^/]+)\/infer$/)
      if (req.method === 'POST' && inferMatch) {
        const name = decodeURIComponent(inferMatch[1])
        const session = registry.get(name)
        if (!session) return send(res, 404, { error: `unknown agent: ${name}` })

        const body = await readJsonBody(req)
        if (typeof body.message !== 'string' || body.message.length === 0) {
          return send(res, 400, { error: 'message (string) required' })
        }

        const result = await session.infer(body.message)
        return send(res, 200, { agent: name, text: result.text, usage: result.usage, stop_reason: result.stop_reason })
      }

      send(res, 404, { error: `not found: ${req.method} ${url.pathname}` })
    } catch (err) {
      send(res, 500, { error: err.message })
    }
  }
}
```

- [ ] **Step 4: Run tests to confirm pass**

```bash
cd "/Users/ganeshkhetawat/My OS/console" && npm run broker:test 2>&1 | tail -15
```

Expected: 17 tests pass.

- [ ] **Step 5: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add console/server/broker/routes.js console/server/broker/test/routes.test.js && git commit -m "feat(broker): HTTP routes for infer/list/health with concurrent dispatch"
```

---

## Task 7: Server bootstrap (index.js)

**Files:**
- Create: `console/server/broker/index.js`

- [ ] **Step 1: Implement index.js**

Create `console/server/broker/index.js`:

```javascript
import { createServer } from 'node:http'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import Anthropic from '@anthropic-ai/sdk'
import { loadManifest } from './manifest.js'
import { AgentRegistry } from './registry.js'
import { createHandler } from './routes.js'

const __dirname = dirname(fileURLToPath(import.meta.url))
const PORT = Number(process.env.BROKER_PORT ?? 3101)
const MANIFEST_PATH = process.env.BROKER_MANIFEST
  ?? resolve(__dirname, '../../../oxide-os/swarmscope_bridge_schema.json')

const apiKey = process.env.ANTHROPIC_API_KEY
if (!apiKey) {
  console.error('[broker] ANTHROPIC_API_KEY not set — exiting')
  process.exit(1)
}

let manifest
try {
  manifest = loadManifest(MANIFEST_PATH)
} catch (err) {
  console.error(`[broker] failed to load manifest at ${MANIFEST_PATH}: ${err.message}`)
  console.error('[broker] hint: set BROKER_MANIFEST to a real manifest path, e.g. server/broker/examples/manifest-2agents.json')
  process.exit(1)
}

const client = new Anthropic({ apiKey })
const registry = AgentRegistry.fromManifest(manifest, client)
const handler = createHandler(registry)

createServer(handler).listen(PORT, () => {
  console.log(`[broker] listening on http://127.0.0.1:${PORT}`)
  console.log(`[broker] manifest: ${MANIFEST_PATH}`)
  console.log(`[broker] provisioned agents (${registry.size}): ${registry.list().map((a) => a.name).join(', ')}`)
})
```

- [ ] **Step 2: Sanity-check the bootstrap path**

The repo schema file is a JSON schema definition (not a runtime manifest), so the default path will fail validation — that's expected. We point the demo at the fixture instead:

```bash
cd "/Users/ganeshkhetawat/My OS/console" && BROKER_MANIFEST=server/broker/examples/manifest-2agents.json ANTHROPIC_API_KEY=fake-key-for-startup-test timeout 2 node server/broker/index.js 2>&1 | head -10
```

Expected output includes:
```
[broker] listening on http://127.0.0.1:3101
[broker] provisioned agents (2): alice, bob
```
(Process exits via timeout after 2s — that's fine.)

- [ ] **Step 3: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add console/server/broker/index.js && git commit -m "feat(broker): bootstrap server on :3101 with Anthropic client"
```

---

## Task 8: Live integration smoke test (gated by API key)

**Files:**
- Create: `console/server/broker/test/live.test.js`

- [ ] **Step 1: Write the gated live test**

Create `console/server/broker/test/live.test.js`:

```javascript
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import Anthropic from '@anthropic-ai/sdk'
import { loadManifest } from '../manifest.js'
import { AgentRegistry } from '../registry.js'
import { createHandler } from '../routes.js'

const __dirname = dirname(fileURLToPath(import.meta.url))
const FIXTURE = resolve(__dirname, '../examples/manifest-2agents.json')

const skipLive = !process.env.ANTHROPIC_API_KEY

test('LIVE: end-to-end real Claude call through broker', { skip: skipLive ? 'set ANTHROPIC_API_KEY to run' : false }, async () => {
  const client = new Anthropic({ apiKey: process.env.ANTHROPIC_API_KEY })
  const registry = AgentRegistry.fromManifest(loadManifest(FIXTURE), client)
  const srv = createServer(createHandler(registry))
  await new Promise((r) => srv.listen(0, '127.0.0.1', r))
  const { port } = srv.address()

  const [a, b] = await Promise.all([
    fetch(`http://127.0.0.1:${port}/v1/agent/alice/infer`, {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ message: 'Say the word PING and nothing else.' }),
    }).then((r) => r.json()),
    fetch(`http://127.0.0.1:${port}/v1/agent/bob/infer`, {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ message: 'Summarize: the sky is blue.' }),
    }).then((r) => r.json()),
  ])

  assert.equal(a.agent, 'alice')
  assert.ok(a.text.length > 0, 'alice returned text')
  assert.ok(a.usage.output_tokens > 0)
  assert.equal(b.agent, 'bob')
  assert.ok(b.text.length > 0, 'bob returned text')
  srv.close()
})
```

- [ ] **Step 2: Run the live test (only if you have ANTHROPIC_API_KEY)**

```bash
cd "/Users/ganeshkhetawat/My OS/console" && ANTHROPIC_API_KEY="$ANTHROPIC_API_KEY" npm run broker:test 2>&1 | tail -10
```

Expected (with key set): 18 tests pass, live test included. Without key: 17 pass, live test skipped with message "set ANTHROPIC_API_KEY to run".

- [ ] **Step 3: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add console/server/broker/test/live.test.js && git commit -m "test(broker): gated live integration test against real Claude API"
```

---

## Task 9: Final verification + README snippet

**Files:**
- Modify: `console/package.json` (already done in Task 1)
- Create: `console/server/broker/README.md`

- [ ] **Step 1: Run full test suite**

```bash
cd "/Users/ganeshkhetawat/My OS/console" && npm run broker:test 2>&1 | tail -20
```

Expected: all tests pass (17 without live key, 18 with).

- [ ] **Step 2: Write broker README**

Create `console/server/broker/README.md`:

```markdown
# Oxide Host Broker

HTTP service on :3101 that turns Oxide kernel agents into real Claude agents.

## Run

```bash
export ANTHROPIC_API_KEY=sk-ant-...
export BROKER_MANIFEST=server/broker/examples/manifest-2agents.json
npm run broker
```

## Endpoints

- `GET  /v1/health` — `{ status, agent_count }`
- `GET  /v1/agents` — `{ agents: [{ name, role, model, history_length }] }`
- `POST /v1/agent/:name/infer` — body `{ message }` → `{ agent, text, usage, stop_reason }`

## Test

```bash
npm run broker:test                          # stubbed, no API calls
ANTHROPIC_API_KEY=sk-... npm run broker:test # includes live test
```

## Environment

| Var | Default | Purpose |
|---|---|---|
| `ANTHROPIC_API_KEY` | required | Claude API key |
| `BROKER_PORT` | `3101` | HTTP port |
| `BROKER_MANIFEST` | repo schema path | SwarmScope bridge manifest |
```

- [ ] **Step 3: Commit**

```bash
cd "/Users/ganeshkhetawat/My OS" && git add console/server/broker/README.md && git commit -m "docs(broker): add run/test/env reference"
```

---

## Done criteria

- `npm run broker:test` passes (17+ tests).
- `ANTHROPIC_API_KEY=... npm run broker` starts, prints `provisioned agents (2): alice, bob`, listens on 3101.
- `curl -X POST :3101/v1/agent/alice/infer -d '{"message":"hi"}' -H 'content-type: application/json'` returns a real Claude reply.
- Concurrent POSTs to alice and bob return distinct, non-interleaved responses.

## Out of scope (next plan)

- Kernel-side changes to `oxide-os/kernel/src/swarmscope/loader.rs` to actually POST from inside QEMU.
- Aggregator wiring via IPC.
- Discord/Slack/etc. channel adapters.
- TLS in kernel.
- Persistent history.
