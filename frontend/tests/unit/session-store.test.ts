import { describe, expect, it, vi } from 'vitest'
import { SessionStore, effectiveAge, orderBlockReason } from '../../src/client/session-store'
import type { LiveData, LocalGateway, SessionData } from '../../src/client/gateway'
import type { OrderSummary } from '../../src/client/candid-codec'
import { CanisterError } from '../../src/client/result'

const deferred = <T>() => {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((yes, no) => {
    resolve = yes
    reject = no
  })
  return { promise, resolve, reject }
}
const data = () =>
  ({
    funds: { revision: 0n, recovery_fence: [] },
    fundEvents: { items: [], next_cursor: [] },
    snapshot: { data_age_ms: 0n, account_id: new Uint8Array([1]), revision: 0n },
    agent: { current: [{ state: { Active: null }, expires_at: [] }] },
    eligibility: { eligible: true, terms_version: 1n, expires_at: [100000n] },
    vaultCycles: { new_risk_stopped: false },
    coreCycles: { new_risk_stopped: false },
    btcMarket: { market: 'BTC', eligible_for_new_risk: true },
    ethMarket: { market: 'ETH', eligible_for_new_risk: true },
    vaultJournal: [false, false],
    coreJournal: [false, false],
    orders: { items: [], next_cursor: [] },
    issues: [],
  }) as unknown as LiveData
const input = {
  market: 'BTC',
  side: 'buy' as const,
  kind: 'limit' as const,
  quantity: '0.001',
  price: '60000',
}
const order = {
  order_id: new Uint8Array(32).fill(1),
  state: { Filled: null },
} as unknown as OrderSummary
function fixture() {
  let now = 0
  const gateway = {
    login: vi.fn(async () => ({ address: 'user-a' }) as SessionData),
    logout: vi.fn(async () => {}),
    prepareTradingAccount: vi.fn<LocalGateway['prepareTradingAccount']>(
      async () => new Uint8Array(32),
    ),
    refresh: vi.fn(async () => data()),
    fundingInstructions: vi.fn<LocalGateway['fundingInstructions']>(),
    lookupOrder: vi.fn<LocalGateway['lookupOrder']>(async () => undefined),
    submitOrder: vi.fn<LocalGateway['submitOrder']>(async (args) => ({
      request_id: args.clientRequestId!,
      order_id: order.order_id,
      cloid: new Uint8Array(16),
      accepted_at: 0n,
    })),
  }
  const store = new SessionStore(gateway, () => now)
  return {
    store,
    gateway,
    advance: (ms: number) => {
      now += ms
    },
  }
}
describe('session lifecycle and order reconciliation', () => {
  it('blocks orders while a restored journal needs reconciliation', async () => {
    const { store, gateway } = fixture()
    await store.login()
    gateway.refresh.mockResolvedValueOnce({ ...data(), coreJournal: [true, true] })
    await store.refresh()
    expect(orderBlockReason(store.getSnapshot(), 0)).toContain('restored records')
    await store.submit(input)
    expect(gateway.submitOrder).not.toHaveBeenCalled()
  })
  it('blocks new risk when recovery is fenced or Agent approval expired', async () => {
    const { store, gateway } = fixture()
    await store.login()
    gateway.refresh.mockResolvedValueOnce({
      ...data(),
      funds: { ...data().funds, recovery_fence: [{ Preparing: null }] },
    })
    await store.refresh()
    expect(orderBlockReason(store.getSnapshot(), 0, 1_000)).toContain('Recovery is fenced')
    await store.submit(input)
    expect(gateway.submitOrder).not.toHaveBeenCalled()
    gateway.refresh.mockResolvedValueOnce({
      ...data(),
      agent: { ...data().agent!, current: [{ ...data().agent!.current[0]!, expires_at: [999n] }] },
    })
    await store.refresh()
    expect(orderBlockReason(store.getSnapshot(), 0, 1_000)).toContain('expired')
  })
  it('does not send invalid input and clears tracked requests on logout', async () => {
    const { store, gateway } = fixture()
    await store.login()
    await store.submit({ ...input, quantity: '0' })
    expect(gateway.submitOrder).not.toHaveBeenCalled()
    gateway.submitOrder.mockRejectedValueOnce(new Error('unknown'))
    await store.submit(input)
    expect(store.getSnapshot().orders).toHaveLength(1)
    await store.logout()
    expect(store.getSnapshot().orders).toHaveLength(0)
    expect(store.getSnapshot().error).toBeUndefined()
  })
  it('blocks partial refresh failures and ignores late operation completion after logout', async () => {
    const { store, gateway } = fixture()
    await store.login()
    gateway.refresh.mockResolvedValueOnce({
      ...data(),
      issues: [{ source: 'orders', message: 'unavailable' }],
      orders: undefined,
    })
    await store.refresh()
    expect(orderBlockReason(store.getSnapshot(), 0)).toContain('Could not refresh')
    await store.refresh()
    const pending = deferred<void>()
    const action = store.run(() => pending.promise)
    await store.logout()
    await store.login()
    const latest = store.getSnapshot()
    pending.reject(new Error('old operation failed'))
    await action
    expect(store.getSnapshot()).toBe(latest)
  })
  it('ages monotonically and checks freshness again immediately before submission', async () => {
    const { store, gateway, advance } = fixture()
    await store.login()
    expect(effectiveAge(store.getSnapshot(), 10000)).toBe(10000)
    expect(orderBlockReason(store.getSnapshot(), 10000)).toBeUndefined()
    advance(10001)
    await store.submit(input)
    expect(gateway.submitOrder).not.toHaveBeenCalled()
    expect(store.getSnapshot().error).toContain('stale')
  })
  it('shares refresh, drops delayed responses across logout and re-login', async () => {
    const { store, gateway } = fixture()
    await store.login()
    const pending = deferred<LiveData>()
    gateway.refresh.mockReturnValueOnce(pending.promise)
    const old = store.refresh()
    expect(store.refresh()).toBe(old)
    await store.logout()
    expect(store.getSnapshot().data).toBeUndefined()
    gateway.login.mockResolvedValueOnce({ address: 'user-b' } as SessionData)
    await store.login()
    const latest = store.getSnapshot().data
    pending.resolve(data())
    await old
    expect(store.getSnapshot().address).toBe('user-b')
    expect(store.getSnapshot().data).toBe(latest)
  })
  it('ignores old errors and does not revoke the new session', async () => {
    const { store, gateway } = fixture()
    await store.login()
    const pending = deferred<LiveData>()
    gateway.refresh.mockReturnValueOnce(pending.promise)
    const old = store.refresh()
    await store.logout()
    await store.login()
    pending.reject(new CanisterError('SessionRevoked', null))
    await old
    expect(store.getSnapshot().address).toBe('user-a')
    expect(store.getSnapshot().refreshError).toBeUndefined()
  })
  it('blocks on refresh failure, recovers without clearing action errors', async () => {
    const { store, gateway } = fixture()
    await store.login()
    await store.run(async () => {
      throw new Error('action rejected')
    })
    gateway.refresh.mockRejectedValueOnce(new Error('offline'))
    await store.refresh()
    expect(orderBlockReason(store.getSnapshot(), 0)).toContain('Could not refresh')
    await store.refresh()
    expect(orderBlockReason(store.getSnapshot(), 0)).toBeUndefined()
    expect(store.getSnapshot().error).toBe('action rejected')
  })
  it('waits for pre-action refresh then fetches new state; prevents double actions', async () => {
    const { store, gateway } = fixture()
    await store.login()
    const pending = deferred<LiveData>()
    gateway.refresh.mockReturnValueOnce(pending.promise)
    const before = store.refresh()
    const action = vi.fn(async () => {})
    const run = store.run(action)
    await store.run(action)
    expect(action).toHaveBeenCalledTimes(1)
    pending.resolve(data())
    await before
    await run
    expect(gateway.refresh).toHaveBeenCalledTimes(3)
  })
  it('retains accepted status when subsequent refresh fails', async () => {
    const { store, gateway } = fixture()
    await store.login()
    gateway.refresh.mockRejectedValueOnce(new Error('offline'))
    await store.submit(input)
    expect(store.getSnapshot().orders[0].state).toBe('accepted')
    expect(store.getSnapshot().refreshError).toBe('offline')
  })
  it('keeps unknown blocked until found, without resending the order', async () => {
    const { store, gateway } = fixture()
    await store.login()
    gateway.submitOrder.mockRejectedValueOnce(new Error('response lost'))
    await store.submit(input)
    await store.refresh()
    expect(store.getSnapshot().orders[0].state).toBe('unknown')
    await store.submit(input)
    expect(gateway.submitOrder).toHaveBeenCalledTimes(1)
    gateway.lookupOrder.mockResolvedValueOnce(order)
    await store.refresh()
    expect(store.getSnapshot().orders[0].state).toBe('accepted')
    expect(orderBlockReason(store.getSnapshot(), 0)).toBeUndefined()
    expect(gateway.submitOrder).toHaveBeenCalledTimes(1)
  })
  it('marks explicit rejection and never shows it as pending', async () => {
    const { store, gateway } = fixture()
    await store.login()
    gateway.submitOrder.mockRejectedValueOnce(new CanisterError('BadRequest', 'bad quantity'))
    await store.submit(input)
    expect(store.getSnapshot().orders[0].state).toBe('rejected')
    expect(orderBlockReason(store.getSnapshot(), 0)).toBeUndefined()
  })
  it('treats a mismatched receipt as unknown and preserves venue Unknown after lookup', async () => {
    const { store, gateway } = fixture()
    await store.login()
    gateway.submitOrder.mockResolvedValueOnce({
      request_id: [],
      order_id: [],
      cloid: [],
      accepted_at: 0n,
    })
    await store.submit(input)
    expect(store.getSnapshot().orders[0].state).toBe('unknown')
    gateway.lookupOrder.mockResolvedValueOnce({ ...order, state: { Unknown: null } })
    await store.refresh()
    expect(orderBlockReason(store.getSnapshot(), 0)).toContain('Reconciling')
  })
})

describe('funding instructions session ownership', () => {
  const instructions = (source: number) =>
    ({
      source_hl_account_address: new Uint8Array(20).fill(source),
      hl_account_address: new Uint8Array(20).fill(9),
      asset: { Usdc: null },
      network: { Testnet: null },
      account_kind: { Reserve: null },
      minimum_amount: [],
      memo_required: false,
    }) as Awaited<ReturnType<LocalGateway['fundingInstructions']>>

  it('clears displayed instructions on logout and a new login', async () => {
    const { store, gateway } = fixture()
    gateway.fundingInstructions.mockResolvedValue(instructions(1))
    await store.login()
    await store.loadFundingInstructions()
    expect(store.getSnapshot().fundingInstructions).toEqual(instructions(1))
    await store.logout()
    expect(store.getSnapshot().fundingInstructions).toBeUndefined()
    gateway.login.mockResolvedValue({ address: 'user-b' } as SessionData)
    await store.login()
    expect(store.getSnapshot().fundingInstructions).toBeUndefined()
  })

  it('discards the old response when it arrives after the new account response', async () => {
    const { store, gateway } = fixture()
    const pending = deferred<Awaited<ReturnType<LocalGateway['fundingInstructions']>>>()
    gateway.fundingInstructions.mockReturnValueOnce(pending.promise)
    await store.login()
    const old = store.loadFundingInstructions()
    await store.logout()
    gateway.login.mockResolvedValue({ address: 'user-b' } as SessionData)
    await store.login()
    gateway.fundingInstructions.mockResolvedValue(instructions(2))
    await store.loadFundingInstructions()
    pending.resolve(instructions(1))
    await old
    expect(store.getSnapshot().fundingInstructions).toEqual(instructions(2))
  })
})
