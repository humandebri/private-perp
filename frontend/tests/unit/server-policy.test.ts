import { describe, expect, it } from 'vitest'
import { gateRequest, secureResponse } from '../../src/server-policy'
describe('edge boundary', () => {
  it('rejects financial POSTs without parsing them', () => {
    expect(
      gateRequest(
        new Request('https://demo.test/trade', { method: 'POST', body: 'secret' }),
        'demo',
        'US',
      )?.status,
    ).toBe(405)
  })
  it('cannot accidentally run as a live service', () => {
    expect(gateRequest(new Request('https://demo.test'), 'mainnet', 'US')?.status).toBe(503)
  })
  it('uses platform country metadata, not spoofable request headers', () => {
    const request = Object.assign(
      new Request('https://demo.test', { headers: { 'CF-IPCountry': 'JP' } }),
      { cf: { country: 'US' } },
    )
    expect(gateRequest(request, 'demo', 'US')?.status).toBe(403)
  })
  it('permits a local demo with no country metadata', () => {
    expect(gateRequest(new Request('https://demo.test'), 'demo', 'US')).toBeNull()
  })
  it('sets security and non-cache headers', () => {
    const response = secureResponse(new Response('public'))
    expect(response.headers.get('cache-control')).toBe('no-store')
    expect(response.headers.get('referrer-policy')).toBe('no-referrer')
    expect(response.headers.get('content-security-policy')).toContain("frame-ancestors 'none'")
  })
})
