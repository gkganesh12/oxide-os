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
