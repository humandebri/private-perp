import { expect, it, vi } from 'vitest'
import { LocalGateway } from '../../src/client/gateway'
import type { FundStatus } from '../../src/client/candid/funds_vault.did.js'

const balance = (reserve: bigint, trading = 0n): FundStatus => ({
  reserve_unallocated: reserve,
  withdrawable: reserve,
  trading_equity: trading,
  in_transit: 0n,
  reserved_for_withdrawal: 0n,
  trading_unrealized_pnl: 0n,
  unknowns: [],
  recovery_fence: [],
  observed_at: 1n,
  revision: 0n,
})

function fixture(before: FundStatus, after: FundStatus) {
  const gateway = new LocalGateway()
  const read = vi.fn().mockResolvedValueOnce({ Ok: before }).mockResolvedValue({ Ok: after })
  Reflect.set(gateway, 'active', { session: {}, clients: { vault: { get_fund_status: read } } })
  const seed = vi.spyOn(gateway, 'seedDeposit').mockResolvedValue(undefined)
  const allocate = vi.spyOn(gateway, 'allocate').mockResolvedValue({
    request_id: new Uint8Array(32),
    fund_action_id: [],
    state: { Accepted: null },
    accepted_at: 1n,
  })
  return { gateway, seed, allocate }
}

it('deposit completes with a held reserve and never allocates automatically', async () => {
  const { gateway, seed, allocate } = fixture(balance(0n), balance(100_000_000n))
  await gateway.depositToReserve('100', 100_000_000n)
  expect(seed).toHaveBeenCalledExactlyOnceWith('100')
  expect(allocate).not.toHaveBeenCalled()
})

it('later use allocates only the chosen amount without another deposit', async () => {
  const { gateway, seed, allocate } = fixture(
    balance(100_000_000n),
    balance(80_000_000n, 20_000_000n),
  )
  await gateway.allocateFromReserve(20_000_000n)
  expect(seed).not.toHaveBeenCalled()
  expect(allocate).toHaveBeenCalledExactlyOnceWith(20_000_000n)
})
