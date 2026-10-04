import { describe, expect, it } from 'vitest'
import {
  gateRequest,
  inlineScriptHashes,
  secureRenderedResponse,
  secureResponse,
} from '../../src/server-policy'

describe('edge boundary', () => {
  it('rejects financial POSTs without parsing them', () => {
    expect(
      gateRequest(
        new Request('http://127.0.0.1/trade', { method: 'POST', body: 'secret' }),
        'local',
      )?.status,
    ).toBe(405)
  })
  it('cannot accidentally run as a live service', () => {
    expect(gateRequest(new Request('http://127.0.0.1'), 'mainnet')?.status).toBe(503)
  })
  it('allows local stage only on loopback', () => {
    expect(gateRequest(new Request('http://127.0.0.1/trade'), 'local')).toBeNull()
    expect(gateRequest(new Request('http://localhost/trade'), 'local')).toBeNull()
    expect(gateRequest(new Request('https://private-perp.example/trade'), 'local')?.status).toBe(
      403,
    )
  })
  it('serves testnet UI publicly but still blocks financial POSTs', () => {
    expect(gateRequest(new Request('https://private-perp.example/funds'), 'testnet')).toBeNull()
    expect(
      gateRequest(new Request('https://private-perp.example/funds', { method: 'POST' }), 'testnet')
        ?.status,
    ).toBe(405)
  })
  it('sets security and non-cache headers', () => {
    const response = secureResponse(new Response('public'), 'no-store', [
      'http://127.0.0.1:18100',
      'http://127.0.0.1:8080',
    ])
    expect(response.headers.get('cache-control')).toBe('no-store')
    expect(response.headers.get('referrer-policy')).toBe('no-referrer')
    expect(response.headers.get('x-frame-options')).toBe('DENY')
    const csp = response.headers.get('content-security-policy') ?? ''
    expect(csp).toContain("frame-ancestors 'none'")
    expect(csp).toContain("default-src 'self'")
    expect(csp).toContain("script-src 'self'")
    expect(csp).toContain("connect-src 'self'")
    expect(csp).toContain('http://127.0.0.1:18100')
    expect(csp).toContain('http://127.0.0.1:8080')
    expect(csp).toContain("img-src 'self' data:")
    expect(csp).toContain("style-src 'self' 'unsafe-inline'")
    expect(csp).toContain("object-src 'none'")
  })
  it('lets immutable assets opt into a cacheable response without caching HTML', () => {
    const asset = secureResponse(
      new Response('body {}', { headers: { 'content-type': 'text/javascript' } }),
      'public, max-age=31536000, immutable',
    )
    expect(asset.headers.get('cache-control')).toBe('public, max-age=31536000, immutable')
    expect(asset.headers.get('content-type')).toBe('text/javascript')
    expect(asset.headers.get('x-content-type-options')).toBe('nosniff')
  })
})
describe('inline script allow-list', () => {
  it('hashes inline scripts and ignores external ones', async () => {
    const html =
      '<script>alert(1)</script><script src="/assets/a.js"></script><script>let a = 1 && 2</script>'
    expect(await inlineScriptHashes(html)).toEqual([
      "'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
      "'sha256-0yk84dgf0qHU/iaOKvN1/w8T1kTf52JE3tOsN4bJN8o='",
    ])
  })
  it('hashes the text the parser delivers, not the raw bytes', async () => {
    // A literal NUL becomes U+FFFD and CRLF becomes LF before CSP sees the text.
    expect(await inlineScriptHashes('<script>a\u0000b\r\nc</script>')).toEqual([
      "'sha256-yiD46gfUBsL8tmyIcj5s4C8KmtjP/tv8vLmMXIfMlIY='",
    ])
  })
  it('keeps script-src off unsafe-inline while the style attribute still works', async () => {
    const html = new Response('<html><body><script>x()</script></body></html>', {
      headers: { 'content-type': 'text/html; charset=utf-8', 'content-length': '999' },
    })
    const secured = await secureRenderedResponse(html)
    expect(secured.headers.get('cache-control')).toBe('no-store')
    expect(secured.headers.get('content-length')).toBeNull()
    const csp = secured.headers.get('content-security-policy') ?? ''
    const scriptSrc = csp.split('; ').find((directive) => directive.startsWith('script-src')) ?? ''
    expect(scriptSrc).toMatch(/^script-src 'self' 'sha256-\S+'$/)
    expect(scriptSrc).not.toContain('unsafe-inline')
    expect(csp).toContain("style-src 'self' 'unsafe-inline'")
    expect(await secured.text()).toBe('<html><body><script>x()</script></body></html>')
  })
  it('leaves non-HTML responses on the static policy', async () => {
    const secured = await secureRenderedResponse(
      new Response('{}', { headers: { 'content-type': 'application/json' } }),
    )
    expect(secured.headers.get('content-security-policy')).toContain("script-src 'self'")
    expect(secured.headers.get('content-security-policy')).not.toMatch(/script-src 'self' 'sha256/)
    expect(await secured.text()).toBe('{}')
  })
})
