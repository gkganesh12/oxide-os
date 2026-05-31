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
