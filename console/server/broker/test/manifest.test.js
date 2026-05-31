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
