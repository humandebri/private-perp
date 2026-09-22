import handler from '@tanstack/react-start/server-entry'
import { gateRequest, secureRenderedResponse, secureResponse } from './server-policy'

/** Hashed Vite assets are content-addressed and safe to cache forever. */
const IMMUTABLE_ASSET_CACHE = 'public, max-age=31536000, immutable'

export default {
  async fetch(request, env) {
    const connectSources = [env.IC_HOST, env.MOCK_HL_URL, env.MARKET_WS_URL]
    const gate = gateRequest(request, env.APP_STAGE)
    if (gate) return secureResponse(gate, 'no-store', connectSources)
    const pathname = new URL(request.url).pathname
    if (pathname.startsWith('/assets/'))
      return secureResponse(await env.ASSETS.fetch(request), IMMUTABLE_ASSET_CACHE, connectSources)
    if (pathname === '/favicon.ico')
      return secureResponse(new Response(null, { status: 204 }), 'no-store', connectSources)
    return secureRenderedResponse(await handler.fetch(request), connectSources)
  },
} satisfies ExportedHandler<Env>
