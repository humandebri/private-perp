// The edge never parses a financial request or acts as a trading proxy.
export function gateRequest(
  request: Request & { cf?: { country?: string } },
  stage: string,
  blocked: string,
): Response | null {
  if (stage !== 'demo') return new Response('Live service is not configured.', { status: 503 })
  if (!['GET', 'HEAD'].includes(request.method))
    return new Response('Method not allowed', { status: 405, headers: { Allow: 'GET, HEAD' } })
  const country = request.cf?.country
  if (
    country &&
    blocked
      .split(',')
      .map((code) => code.trim())
      .includes(country)
  ) {
    return new Response('This interface is unavailable in your region.', { status: 403 })
  }
  return null
}

export function secureResponse(response: Response): Response {
  const headers = new Headers(response.headers)
  headers.set('X-Content-Type-Options', 'nosniff')
  headers.set('X-Frame-Options', 'DENY')
  headers.set('Referrer-Policy', 'no-referrer')
  headers.set('Permissions-Policy', 'camera=(), microphone=(), geolocation=()')
  headers.set('Cache-Control', 'no-store')
  headers.set(
    'Content-Security-Policy',
    "frame-ancestors 'none'; base-uri 'self'; object-src 'none'; form-action 'none'",
  )
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  })
}
