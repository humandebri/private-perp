// The edge never parses a financial request or acts as a trading proxy.
/** Executable inline scripts are allow-listed by hash, so script-src stays free of 'unsafe-inline'. */
const INLINE_SCRIPT_PATTERN = /<script\b([^>]*)>([\s\S]*?)<\/script\s*>/gi
function contentSecurityPolicy(scriptHashes: string[], connectSources: string[] = []): string {
  return [
    "default-src 'self'",
    ["script-src 'self'", ...scriptHashes].join(' '),
    ["connect-src 'self'", ...connectSources].join(' '),
    "img-src 'self' data:",
    // React writes layout through the `style` attribute (chart sizing, depth bars),
    // which CSP only allows with 'unsafe-inline' on style-src.
    "style-src 'self' 'unsafe-inline'",
    "frame-ancestors 'none'",
    "base-uri 'self'",
    "object-src 'none'",
    "form-action 'none'",
  ].join('; ')
}
/**
 * Applies the HTML parser's input preprocessing (newline normalisation and NUL
 * replacement) so a hash matches the text the browser actually checks.
 */
function preprocessHtml(html: string): string {
  // split/join keeps the NUL replacement out of a control-character regex.
  return html.replace(/\r\n?/g, '\n').split('\u0000').join('\uFFFD')
}
async function sha256Base64(text: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text))
  return btoa(String.fromCharCode(...new Uint8Array(digest)))
}
/** Hashes every executable inline script so `script-src` can allow exactly those. */
export async function inlineScriptHashes(html: string): Promise<string[]> {
  const hashes: string[] = []
  for (const [, attributes = '', source = ''] of preprocessHtml(html).matchAll(
    INLINE_SCRIPT_PATTERN,
  )) {
    if (/\bsrc\s*=/i.test(attributes)) continue
    hashes.push(`'sha256-${await sha256Base64(source)}'`)
  }
  return hashes
}
function securityHeaders(source: Headers): Headers {
  const headers = new Headers(source)
  headers.set('X-Content-Type-Options', 'nosniff')
  headers.set('X-Frame-Options', 'DENY')
  headers.set('Referrer-Policy', 'no-referrer')
  headers.set('Permissions-Policy', 'camera=(), microphone=(), geolocation=()')
  return headers
}
export function gateRequest(request: Request, stage: string): Response | null {
  if (stage !== 'local') return new Response('Local service is not configured.', { status: 503 })
  const hostname = new URL(request.url).hostname
  if (!['127.0.0.1', 'localhost', '::1'].includes(hostname)) {
    return new Response('Loopback access only.', { status: 403 })
  }
  if (!['GET', 'HEAD'].includes(request.method))
    return new Response('Method not allowed', { status: 405, headers: { Allow: 'GET, HEAD' } })
  return null
}

export function secureResponse(
  response: Response,
  cacheControl = 'no-store',
  connectSources: string[] = [],
): Response {
  const headers = securityHeaders(response.headers)
  headers.set('Cache-Control', cacheControl)
  headers.set('Content-Security-Policy', contentSecurityPolicy([], connectSources))
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  })
}

/**
 * Server-rendered HTML carries framework bootstrap and route data inline, so the
 * policy is derived from the document that is actually served. The body is read
 * once to hash it; non-HTML responses keep the static policy untouched.
 */
export async function secureRenderedResponse(
  response: Response,
  connectSources: string[] = [],
): Promise<Response> {
  const contentType = response.headers.get('content-type') ?? ''
  if (!contentType.includes('text/html'))
    return secureResponse(response, 'no-store', connectSources)
  const html = await response.text()
  const headers = securityHeaders(response.headers)
  headers.set('Cache-Control', 'no-store')
  headers.set(
    'Content-Security-Policy',
    contentSecurityPolicy(await inlineScriptHashes(html), connectSources),
  )
  headers.delete('content-length')
  headers.delete('content-encoding')
  return new Response(html, {
    status: response.status,
    statusText: response.statusText,
    headers,
  })
}
