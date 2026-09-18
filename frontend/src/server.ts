import handler from '@tanstack/react-start/server-entry'
import { gateRequest, secureResponse } from './server-policy'

export default {
  async fetch(request, env) {
    const gate = gateRequest(request, env.APP_STAGE, env.BLOCKED_COUNTRIES)
    if (gate) return secureResponse(gate)
    const pathname = new URL(request.url).pathname
    if (pathname.startsWith('/assets/')) return secureResponse(await env.ASSETS.fetch(request))
    if (pathname === '/favicon.ico') return secureResponse(new Response(null, { status: 204 }))
    return secureResponse(await handler.fetch(request))
  },
} satisfies ExportedHandler<Env>
