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
      messages: [...this.history],
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
