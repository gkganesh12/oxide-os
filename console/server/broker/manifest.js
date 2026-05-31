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
