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
