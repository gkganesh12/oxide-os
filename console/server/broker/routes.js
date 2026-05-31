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
