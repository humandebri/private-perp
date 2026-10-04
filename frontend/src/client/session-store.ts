import { LocalGateway, requestId, type LiveData } from './gateway'
import type { OrderSummary } from './candid-codec'
import { CanisterError, SubmissionNotSentError, variantName } from './result'
import { bytesToHex } from './wallet'

type Gateway = Pick<
  LocalGateway,
  | 'login'
  | 'logout'
  | 'prepareTradingAccount'
  | 'refresh'
  | 'submitOrder'
  | 'lookupOrder'
  | 'fundingInstructions'
>
export type OrderInput = Parameters<LocalGateway['submitOrder']>[0]
export type TrackedOrder = {
  id: Uint8Array
  market: string
  state: 'sending' | 'accepted' | 'rejected' | 'unknown'
  orderId?: string
  order?: OrderSummary
  message?: string
}
export type SessionState = {
  generation: number
  address?: string
  data?: LiveData
  fundingInstructions?: Awaited<ReturnType<LocalGateway['fundingInstructions']>>
  receivedAt?: number
  busy: boolean
  error?: string
  refreshError?: string
  orders: TrackedOrder[]
}
const message = (cause: unknown) => {
  if (cause instanceof CanisterError && cause.code === 'VenueRateLimited')
    return 'Waiting for shared REST capacity. Cancellation, closes and withdrawals remain available.'
  return cause instanceof Error ? cause.message : String(cause)
}
const expired = (cause: unknown) =>
  cause instanceof CanisterError &&
  ['SessionExpired', 'SessionRevoked', 'Unauthenticated'].includes(cause.code)

export function effectiveAge(state: SessionState, now: number): number {
  if (!state.data?.snapshot || state.receivedAt === undefined) return Infinity
  return Number(state.data.snapshot.data_age_ms) + Math.max(0, now - state.receivedAt)
}
export function orderBlockReason(
  state: SessionState,
  now: number,
  wallNow = Date.now(),
): string | undefined {
  const data = state.data
  if (!state.address) return 'Connect MetaMask'
  if (state.refreshError || !data?.snapshot || !data.orders || !data.agent)
    return 'Could not refresh account, order or agent data. Check your connection.'
  if (!data.vaultJournal || !data.coreJournal)
    return 'Could not verify the dispatch journal. New orders are paused.'
  if (data.vaultJournal[0] || data.coreJournal[0])
    return data.vaultJournal[1] || data.coreJournal[1]
      ? 'Reconciling restored records. Order dispatch is paused.'
      : 'Waiting for journal verification. Order dispatch is paused.'
  if (!data.agent.current[0] || variantName(data.agent.current[0].state) !== 'Active')
    return 'Approve the agent'
  if (!data.eligibility?.eligible)
    return 'Eligibility is missing or expired. Cancellation, closes, recovery and withdrawals remain available.'
  if (
    !data.vaultCycles ||
    !data.coreCycles ||
    data.vaultCycles.new_risk_stopped ||
    data.coreCycles.new_risk_stopped
  )
    return 'New actions are paused due to the cycles balance or configured burn floor.'
  if (
    !data.btcMarket ||
    !data.ethMarket ||
    !data.btcMarket.eligible_for_new_risk ||
    !data.ethMarket.eligible_for_new_risk
  )
    return 'Market observation or liquidity requirements are not met. Cancellation and closes remain available.'
  if (data.agent.current[0].expires_at[0] && data.agent.current[0].expires_at[0] <= BigInt(wallNow))
    return 'Agent approval has expired. Approve a new generation.'
  if (data.funds.recovery_fence.length)
    return 'Recovery is fenced. Cancellation and reduce-only closes remain available.'
  if (!data.snapshot.account_id.length) return 'Could not observe the trading account'
  if (
    state.orders.some((order) => ['sending', 'unknown'].includes(order.state)) ||
    [
      ...data.orders.items,
      ...state.orders.flatMap((order) => (order.order ? [order.order] : [])),
    ].some((order) => variantName(order.state) === 'Unknown')
  )
    return 'Reconciling an unknown order outcome. Do not resubmit or reload.'
  if (effectiveAge(state, now) > 10_000)
    return 'Account data is stale. Refresh trading information, or resume stopped monitoring in Stopped work and manual checks.'
}

/** 非同期処理は世代を跨いで状態を書き戻さない。取得は世代内で直列化する。 */
export class SessionStore {
  private state: SessionState = { generation: 0, busy: false, orders: [] }
  private listeners = new Set<() => void>()
  private fetching?: { generation: number; promise: Promise<void> }
  constructor(
    private gateway: Gateway,
    private now = () => performance.now(),
  ) {}
  getSnapshot = () => this.state
  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }
  private publish(patch: Partial<SessionState>) {
    this.state = { ...this.state, ...patch }
    this.listeners.forEach((listener) => listener())
  }
  isCurrent = (generation: number) => generation === this.state.generation
  private reset() {
    this.state = { generation: this.state.generation + 1, busy: false, orders: [] }
    this.fetching = undefined
    this.listeners.forEach((listener) => listener())
  }
  loadFundingInstructions = async () => {
    if (!this.state.address) return
    const generation = this.state.generation
    const fundingInstructions = await this.gateway.fundingInstructions()
    if (this.isCurrent(generation)) this.publish({ fundingInstructions })
  }
  logout = async () => {
    this.reset()
    await this.gateway.logout()
  }
  login = async () => {
    if (this.state.busy || this.state.address) return
    this.reset()
    const generation = this.state.generation
    this.publish({ busy: true })
    try {
      const session = await this.gateway.login()
      if (!this.isCurrent(generation)) return
      this.publish({ address: session.address })
      await this.gateway.prepareTradingAccount()
      if (this.isCurrent(generation)) await this.refresh()
    } catch (cause) {
      if (this.isCurrent(generation)) this.publish({ error: message(cause) })
    } finally {
      if (this.isCurrent(generation)) this.publish({ busy: false })
    }
  }
  refresh = (): Promise<void> => {
    const generation = this.state.generation
    if (!this.state.address) return Promise.resolve()
    if (this.fetching?.generation === generation) return this.fetching.promise
    const promise = this.load(generation)
    this.fetching = { generation, promise }
    void promise.finally(() => {
      if (this.fetching?.promise === promise) this.fetching = undefined
    })
    return promise
  }
  private async load(generation: number) {
    try {
      const data = await this.gateway.refresh()
      if (!this.isCurrent(generation)) return
      const requiredIssue = data.issues.find((issue) =>
        ['snapshot', 'orders', 'agent'].includes(issue.source),
      )
      this.publish({ data, receivedAt: this.now(), refreshError: requiredIssue?.message })
      for (const item of this.state.orders) {
        if (!['accepted', 'unknown'].includes(item.state)) continue
        const listed = data.orders?.items.find(
          (order) => bytesToHex(order.order_id) === item.orderId,
        )
        if (listed) {
          if (variantName(listed.state) === 'Unknown') this.patchOrder(item.id, { order: listed })
          else this.publish({ orders: this.state.orders.filter((entry) => entry.id !== item.id) })
          continue
        }
        if (
          item.order &&
          ['Filled', 'Cancelled', 'Rejected'].includes(variantName(item.order.state))
        )
          continue
        try {
          const order = await this.gateway.lookupOrder(item.id)
          if (!this.isCurrent(generation)) return
          if (order)
            this.patchOrder(item.id, {
              state: 'accepted',
              orderId: bytesToHex(order.order_id),
              order,
              message: undefined,
            })
        } catch (cause) {
          if (!this.isCurrent(generation)) return
          if (expired(cause)) {
            await this.logout()
            return
          }
          this.patchOrder(item.id, { message: `Reconciliation incomplete: ${message(cause)}` })
        }
      }
    } catch (cause) {
      if (!this.isCurrent(generation)) return
      if (expired(cause)) {
        await this.logout()
        return
      }
      this.publish({ refreshError: message(cause) })
    }
  }
  run = async (action: () => Promise<unknown>) => {
    if (this.state.busy || !this.state.address) return
    const generation = this.state.generation
    this.publish({ busy: true, error: undefined })
    try {
      await action()
      if (!this.isCurrent(generation)) return
      const previous = this.fetching?.promise
      if (previous) await previous
      if (this.isCurrent(generation)) await this.refresh()
    } catch (cause) {
      if (!this.isCurrent(generation)) return
      if (expired(cause)) {
        await this.logout()
        return
      }
      this.publish({ error: message(cause) })
    } finally {
      if (this.isCurrent(generation)) this.publish({ busy: false })
    }
  }
  private patchOrder(id: Uint8Array, patch: Partial<TrackedOrder>) {
    this.publish({
      orders: this.state.orders.map((order) => (order.id === id ? { ...order, ...patch } : order)),
    })
  }
  submit = async (input: OrderInput) => {
    if (this.state.busy) return
    const reason = orderBlockReason(this.state, this.now())
    if (reason) {
      this.publish({ error: reason })
      return
    }
    if (
      ![input.quantity, input.price].every(
        (value) =>
          /^\d+(\.\d+)?$/.test(value) && Number.isFinite(Number(value)) && Number(value) > 0,
      ) ||
      !Number.isInteger(input.leverage ?? 3) ||
      (input.leverage ?? 3) < 1 ||
      (input.leverage ?? 3) > 5 ||
      (input.kind === 'market' &&
        (!Number.isSafeInteger(input.slippageBps ?? 50) ||
          (input.slippageBps ?? 50) < 0 ||
          (input.slippageBps ?? 50) > 0xffffffff))
    ) {
      this.publish({
        error: 'Check quantity, price, leverage and slippage. No order was sent.',
      })
      return
    }
    const generation = this.state.generation
    const id = requestId()
    this.publish({ orders: [{ id, market: input.market, state: 'sending' }, ...this.state.orders] })
    await this.run(async () => {
      try {
        const result = await this.gateway.submitOrder({ ...input, clientRequestId: id })
        if (!this.isCurrent(generation)) return
        if (
          bytesToHex(new Uint8Array(result.request_id)) !== bytesToHex(id) ||
          result.order_id.length !== 32 ||
          result.cloid.length !== 16
        )
          throw new Error('Acceptance response request_id does not match')
        this.patchOrder(id, {
          state: 'accepted',
          orderId: bytesToHex(new Uint8Array(result.order_id)),
        })
      } catch (cause) {
        if (!this.isCurrent(generation)) return
        const rejected =
          cause instanceof SubmissionNotSentError ||
          (cause instanceof CanisterError &&
            [
              'BadRequest',
              'NotAllowed',
              'RiskLimitExceeded',
              'InsufficientFunds',
              'NotEligible',
              'PolicyUnavailable',
              'JournalWriterBusy',
              'StaleAccountState',
              'VenueRateLimited',
              'SigningQueueFull',
              'ReservationConflict',
              'SessionExpired',
              'SessionRevoked',
              'Unauthenticated',
            ].includes(cause.code))
        this.patchOrder(id, { state: rejected ? 'rejected' : 'unknown', message: message(cause) })
        throw cause
      }
    })
  }
}
