import { describe, expect, it, vi } from 'vitest'
import { prepareWithdrawal, waitForFunds } from '../../src/client/fund-flow'
import type { FundStatus } from '../../src/client/candid/funds_vault.did.js'

const funds = (withdrawable: bigint, extras: Partial<FundStatus> = {}): FundStatus => ({
  withdrawable,
  reserve_unallocated: withdrawable,
  in_transit: 0n,
  reserved_for_withdrawal: 0n,
  trading_equity: 100n,
  trading_unrealized_pnl: 0n,
  observed_at: 1n,
  revision: 0n,
  unknowns: [],
  recovery_fence: [],
  ...extras,
})

describe('automatic withdrawal recovery', () => {
  const pause = async () => {}

  it('uses existing reserve without moving trading funds', async () => {
    const recover = vi.fn()
    await prepareWithdrawal(10n, async () => funds(10n), recover, pause)
    expect(recover).not.toHaveBeenCalled()
  })

  it('recovers only the deficit and waits for the fence and balance', async () => {
    const read = vi
      .fn()
      .mockResolvedValueOnce(funds(3n))
      .mockResolvedValueOnce(funds(10n, { recovery_fence: [{ Preparing: null }] }))
      .mockResolvedValueOnce(funds(10n))
    const recover = vi.fn().mockResolvedValue(undefined)
    await prepareWithdrawal(10n, read, recover, pause)
    expect(recover).toHaveBeenCalledExactlyOnceWith(7n)
    expect(read).toHaveBeenCalledTimes(3)
  })

  it('waits for an existing recovery instead of requesting it again', async () => {
    const read = vi
      .fn()
      .mockResolvedValueOnce(funds(0n, { recovery_fence: [{ Preparing: null }] }))
      .mockResolvedValueOnce(funds(10n))
    const recover = vi.fn()
    await prepareWithdrawal(10n, read, recover, pause)
    expect(recover).not.toHaveBeenCalled()
  })

  it('does not resend when a submitted recovery stays unresolved', async () => {
    const recover = vi.fn().mockResolvedValue(undefined)
    await expect(prepareWithdrawal(10n, async () => funds(0n), recover, pause)).rejects.toThrow(
      'Waiting for transfer confirmation',
    )
    expect(recover).toHaveBeenCalledExactlyOnceWith(10n)
  })

  it('stops on unknown transfers before sending another action', async () => {
    const recover = vi.fn()
    const status = funds(0n, {
      unknowns: [
        {
          action_id: new Uint8Array(32),
          kind: { Recovery: null },
          state: { Unknown: null },
          since: 1n,
        },
      ],
    })
    await expect(prepareWithdrawal(10n, async () => status, recover, pause)).rejects.toThrow(
      'Reconciling',
    )
    expect(recover).not.toHaveBeenCalled()
  })
})

it('polls an in-flight POST instead of treating it as an unknown outcome', async () => {
  const read = vi
    .fn()
    .mockResolvedValueOnce(
      funds(10n, {
        unknowns: [
          {
            action_id: new Uint8Array(32),
            kind: { Allocation: null },
            state: { Dispatching: null },
            since: 1n,
          },
        ],
      }),
    )
    .mockResolvedValueOnce(funds(10n))
  const sleep = vi.fn().mockResolvedValue(undefined)
  await waitForFunds(read, (status) => status.withdrawable === 10n, sleep)
  expect(read).toHaveBeenCalledTimes(2)
  expect(sleep).toHaveBeenCalledTimes(1)
})
