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
