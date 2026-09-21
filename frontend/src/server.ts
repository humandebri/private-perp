import handler from '@tanstack/react-start/server-entry'
import { gateRequest, secureRenderedResponse, secureResponse } from './server-policy'

/** Hashed Vite assets are content-addressed and safe to cache forever. */
const IMMUTABLE_ASSET_CACHE = 'public, max-age=31536000, immutable'

export default {
  async fetch(request, env) {
    const gate = gateRequest(request, env.APP_STAGE, env.BLOCKED_COUNTRIES, {
      allowUnknownCountry: env.ALLOW_UNKNOWN_COUNTRY === '1',
    })
    if (gate) return secureResponse(gate)
    const pathname = new URL(request.url).pathname
    if (pathname.startsWith('/assets/'))
      return secureResponse(await env.ASSETS.fetch(request), IMMUTABLE_ASSET_CACHE)
    if (pathname === '/favicon.ico') return secureResponse(new Response(null, { status: 204 }))
    return secureRenderedResponse(await handler.fetch(request))
  },
} satisfies ExportedHandler<Env>
