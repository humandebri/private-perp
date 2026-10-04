import type { FundStatus } from './candid/funds_vault.did.js'

type ReadFunds = () => Promise<FundStatus>
type Pause = () => Promise<void>
const pause: Pause = () => new Promise((resolve) => setTimeout(resolve, 1_000))

function requireKnownFunds(status: FundStatus, allowDispatching = false) {
  if (status.unknowns.some((action) => !allowDispatching || !('Dispatching' in action.state)))
    throw new Error('Reconciling the transfer result. Check history.')
}

/** Poll reads only. A timeout never retries a transfer with a new request ID. */
export async function waitForFunds(
  read: ReadFunds,
  ready: (status: FundStatus) => boolean,
  sleep: Pause = pause,
): Promise<FundStatus> {
  for (let attempt = 0; attempt < 45; attempt++) {
    const status = await read()
    requireKnownFunds(status, true)
    if (!status.unknowns.length && ready(status)) return status
    await sleep()
  }
  throw new Error('Waiting for transfer confirmation. Do not resubmit; check balances and history.')
}

/** Recover only the shortfall, then wait for confirmed spendable reserve. */
export async function prepareWithdrawal(
  amount: bigint,
  read: ReadFunds,
  recover: (amount: bigint) => Promise<unknown>,
  sleep: Pause = pause,
): Promise<void> {
  if (amount <= 0n) throw new Error('Withdrawal amount must be greater than zero.')
  let status = await read()
  requireKnownFunds(status)
  if (status.recovery_fence.length) {
    status = await waitForFunds(read, (funds) => !funds.recovery_fence.length, sleep)
  }
  if (status.withdrawable >= amount) return
  await recover(amount - status.withdrawable)
  await waitForFunds(
    read,
    (funds) => !funds.recovery_fence.length && funds.withdrawable >= amount,
    sleep,
  )
}
