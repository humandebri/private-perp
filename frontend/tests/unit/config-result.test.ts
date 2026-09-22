import { describe, expect, it } from 'vitest'
import { resolveConfig } from '../../src/client/config'
import { optionalValue, type LiveDataIssue } from '../../src/client/gateway'
import { CanisterError, unwrap } from '../../src/client/result'

const valid = {
  VITE_APP_STAGE: 'local',
  VITE_IC_HOST: 'http://127.0.0.1:18100',
  VITE_MOCK_HL_URL: 'http://localhost:8080',
  VITE_MARKET_WS_URL: 'ws://localhost:8080/ws',
  VITE_FUNDS_VAULT_CANISTER_ID: 'aaaaa-aa',
  VITE_TRADING_CORE_CANISTER_ID: 'aaaaa-aa',
}
describe('local configuration and results', () => {
  it('requires every local endpoint and refuses non-loopback', () => {
    expect(resolveConfig(valid).mockHl).toBe('http://localhost:8080')
    expect(() => resolveConfig({ ...valid, VITE_APP_STAGE: 'mainnet' })).toThrow('local')
    expect(() => resolveConfig({ ...valid, VITE_MOCK_HL_URL: 'https://example.com' })).toThrow(
      'loopback',
    )
    expect(() => resolveConfig({ ...valid, VITE_IC_HOST: '' })).toThrow('VITE_IC_HOST')
    expect(() =>
      resolveConfig({ ...valid, VITE_MARKET_WS_URL: 'http://localhost:8080/ws' }),
    ).toThrow('WebSocket')
  })
  it('maps candid results without hiding the error code', () => {
    expect(unwrap({ Ok: 7 })).toBe(7)
    expect(() => unwrap({ Err: { SessionExpired: null } })).toThrow(CanisterError)
    expect(() => unwrap({ Err: { SessionExpired: null } })).toThrow('SessionExpired')
  })
  it('keeps optional live-data failures visible with their source', () => {
    const issues: LiveDataIssue[] = []
    expect(optionalValue({ status: 'fulfilled', value: 7 }, 'snapshot', issues)).toBe(7)
    expect(
      optionalValue(
        { status: 'rejected', reason: new Error('HPKE key mismatch') },
        'orders',
        issues,
      ),
    ).toBeUndefined()
    expect(issues).toEqual([{ source: 'orders', message: 'HPKE key mismatch' }])

    const missing: LiveDataIssue[] = []
    expect(
      optionalValue(
        {
          status: 'rejected',
          reason: new CanisterError('NotAllowed', { code: { AccountNotOwned: null } }),
        },
        'snapshot',
        missing,
      ),
    ).toBeUndefined()
    expect(missing).toEqual([])
  })
})
