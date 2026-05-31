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
