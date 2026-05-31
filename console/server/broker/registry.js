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
