import { Principal } from '@icp-sdk/core/principal'
import type { FundStatus, Paged as FundEvents, SessionHandle } from './candid/funds_vault.did.js'
import type { AgentStatus, SubmitOrderResult } from './candid/trading_core.did.js'
import { codec, type Fill, type OrderSummary, type Page, type Snapshot } from './candid-codec'
import { EnvelopeClient, envelopeAad, newRequestId } from './envelope'
import { createClients, type CanisterClients } from './ic'
import { CanisterError, unwrap } from './result'
import { connectWallet, hexToBytes, signTypedData, withdrawalTypedData } from './wallet'

const nowMs = () => BigInt(Date.now())
export const requestId = () => crypto.getRandomValues(new Uint8Array(32))

export type SessionData = {
  address: string
  session: SessionHandle
  clients: CanisterClients
  envelope: EnvelopeClient
}

export type LiveData = {
  funds: FundStatus
  agent?: AgentStatus
  snapshot?: Snapshot
  orders?: Page<OrderSummary>
  fills?: Page<Fill>
  fundEvents: FundEvents
  issues: LiveDataIssue[]
}

export type LiveDataIssue = {
  source: 'agent' | 'snapshot' | 'orders' | 'fills'
  message: string
}

const errorMessage = (reason: unknown) =>
  reason instanceof Error ? reason.message : String(reason)

const isMissingTradingAccount = (reason: unknown) => {
  if (!(reason instanceof CanisterError) || reason.code !== 'NotAllowed') return false
  const detail = reason.detail as { code?: Record<string, unknown> } | undefined
  return detail?.code && 'AccountNotOwned' in detail.code
}

export function optionalValue<T>(
  result: PromiseSettledResult<T>,
  source: LiveDataIssue['source'],
  issues: LiveDataIssue[],
): T | undefined {
  if (result.status === 'fulfilled') return result.value
  if (isMissingTradingAccount(result.reason)) return undefined
  issues.push({ source, message: errorMessage(result.reason) })
  return undefined
}

export class LocalGateway {
  private active?: SessionData

  get session(): SessionData | undefined {
    return this.active
  }

  async login(): Promise<SessionData> {
    const clients = await createClients()
    const address = await connectWallet()
    const challenge = unwrap(
      await clients.vault.issue_challenge({
        principal: clients.principal,
        origin: location.origin,
        network: { Local: null },
        purpose: { Login: null },
        eoa_address: hexToBytes(address, 20),
      }),
    )
    const signature = await signTypedData(address, new Uint8Array(challenge.typed_data))
    const session = unwrap(
      await clients.vault.open_session({
        challenge_id: challenge.challenge_id,
        eoa_signature: signature,
      }),
    )
    this.active = { address, session, clients, envelope: await EnvelopeClient.create() }
    return this.active
  }

  async logout(): Promise<void> {
    const active = this.active
    this.active = undefined
    if (active) await active.clients.vault.revoke_session(active.session).catch(() => undefined)
  }

  private require(): SessionData {
    if (!this.active) throw new Error('MetaMaskでログインしてください')
    return this.active
  }

  private async sealed<T>(
    method: 'get_account_snapshot' | 'list_orders' | 'list_fills' | 'cancel_order',
    plaintext: Uint8Array,
    decode: (bytes: Uint8Array) => T,
  ): Promise<T> {
    const active = this.require()
    const serverKey = new Uint8Array(unwrap(await active.clients.core.get_hpke_public_key()))
    const id = newRequestId()
    const expiresAt = nowMs() + 60_000n
    const canister = Principal.fromText(active.clients.config.tradingCore)
    const aad = envelopeAad(
      'local',
      canister.toUint8Array(),
      method,
      active.clients.principal.toUint8Array(),
      id,
      expiresAt,
    )
    const response = unwrap(
      await active.clients.core[method]({
        key_id: serverKey,
        network: { Local: null },
        canister,
        method,
        request_id: id,
        expires_at: expiresAt,
        client_public_key: active.envelope.publicKey,
        aad,
        ciphertext: await active.envelope.seal(serverKey, aad, plaintext),
      }),
    )
    if (!id.every((byte, index) => byte === response.request_id[index]))
      throw new Error('応答request_idが一致しません')
    return decode(await active.envelope.open(aad, new Uint8Array(response.ciphertext)))
  }

  async refresh(): Promise<LiveData> {
    const { session, clients } = this.require()
    const funds = unwrap(await clients.vault.get_fund_status(session))
    const fundEvents = unwrap(await clients.vault.list_fund_events(session, [], 100))
    const optional = await Promise.allSettled([
      clients.core.get_agent_status(session).then(unwrap),
      this.sealed('get_account_snapshot', codec.snapshotQuery(session), codec.snapshot),
      this.sealed('list_orders', codec.listQuery(session), codec.orders),
      this.sealed('list_fills', codec.listQuery(session), codec.fills),
    ])
    const issues: LiveDataIssue[] = []
    return {
      funds,
      fundEvents,
      agent: optionalValue(optional[0], 'agent', issues),
      snapshot: optionalValue(optional[1], 'snapshot', issues),
      orders: optionalValue(optional[2], 'orders', issues),
      fills: optionalValue(optional[3], 'fills', issues),
      issues,
    }
  }

  async fundingInstructions() {
    const { session, clients } = this.require()
    try {
      return unwrap(await clients.vault.get_funding_instructions(session))
    } catch {
      unwrap(await clients.vault.provision_reserve_account(session))
      return unwrap(await clients.vault.get_funding_instructions(session))
    }
  }

  async seedDeposit(amount: string): Promise<void> {
    const active = this.require()
    const instructions = await this.fundingInstructions()
    const response = await fetch(`${active.clients.config.mockHl}/admin/deposits`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        address: `0x${[...instructions.hl_account_address].map((value) => value.toString(16).padStart(2, '0')).join('')}`,
        amount,
        id: crypto.randomUUID(),
      }),
    })
    if (!response.ok) throw new Error(`LOCAL MOCK seed失敗: ${response.status}`)
  }

  async allocate(amount: bigint) {
    const { session, clients } = this.require()
    return unwrap(
      await clients.vault.request_allocation({
        session,
        client_request_id: requestId(),
        amount,
        target: { Trading: null },
        intent_signature: [],
      }),
    )
  }

  async recover(amount: bigint) {
    const { session, clients } = this.require()
    return unwrap(await clients.vault.request_recovery(session, requestId(), amount))
  }

  async approveAgent() {
    const { session, clients } = this.require()
    const generation = unwrap(await clients.core.request_agent_generation(session))
    return unwrap(
      await clients.vault.approve_agent_generation(
        session,
        generation.generation,
        generation.agent_address,
      ),
    )
  }

  async submitOrder(args: {
    market: string
    side: 'buy' | 'sell'
    kind: 'market' | 'limit'
    quantity: string
    price: string
  }): Promise<SubmitOrderResult> {
    const { session, clients } = this.require()
    const account = unwrap(await clients.vault.get_trading_account(session))[0]
    if (!account) throw new Error('取引口座がありません。先に配分してください')
    return unwrap(
      await clients.core.submit_order(session, {
        session,
        account_id: account,
        client_request_id: requestId(),
        market: args.market,
        side: args.side === 'buy' ? { Buy: null } : { Sell: null },
        kind: args.kind === 'market' ? { MarketIoc: null } : { LimitGtc: null },
        quantity: args.quantity,
        limit_price: [args.price],
        leverage: [3],
        slippage_tolerance_bps: args.kind === 'market' ? [50] : [],
        reduce_only: false,
        trigger: [],
        expires_after: [nowMs() + 60_000n],
      }),
    )
  }

  async cancel(orderId: Uint8Array | number[]): Promise<void> {
    const { session } = this.require()
    await this.sealed('cancel_order', codec.cancelQuery(session, orderId), codec.empty)
  }

  async withdraw(amount: bigint) {
    const { address, session, clients } = this.require()
    const nonce = nowMs()
    const expiresAt = nonce + 300_000n
    const typedData = withdrawalTypedData({
      address,
      amount,
      nonce,
      expiresAt,
      canister: Principal.fromText(clients.config.fundsVault),
    })
    const signature = await signTypedData(address, typedData)
    return unwrap(
      await clients.vault.request_withdrawal({
        session,
        client_request_id: requestId(),
        amount,
        asset: { Usdc: null },
        destination: { AuthenticatedEoaHlAccount: null },
        network: { Local: null },
        nonce,
        expires_at: expiresAt,
        intent_signature: signature,
      }),
    )
  }
}
