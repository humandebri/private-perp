import { Principal } from '@icp-sdk/core/principal'
import { prepareWithdrawal, waitForFunds } from './fund-flow'
import type {
  CyclesStatus as VaultCycles,
  EligibilityStatus,
  FundStatus,
  Paged as FundEvents,
  SessionHandle,
} from './candid/funds_vault.did.js'
import type {
  AgentStatus,
  CyclesStatus as CoreCycles,
  MarketStatus,
} from './candid/trading_core.did.js'
import {
  codec,
  corePrivateCodec,
  type SubmitOrderResult,
  vaultPrivateCodec,
  type Fill,
  type OrderSummary,
  type Page,
  type Position,
  type Snapshot,
} from './candid-codec'
import { EnvelopeClient, envelopeAad, newRequestId } from './envelope'
import { createClients, type CanisterClients } from './ic'
import { CanisterError, SubmissionNotSentError, unwrap } from './result'
import {
  connectWallet,
  hexToBytes,
  signPersonalBytes,
  signTypedData,
  withdrawalTypedData,
} from './wallet'

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
  eligibility?: EligibilityStatus
  vaultCycles?: VaultCycles
  coreCycles?: CoreCycles
  btcMarket?: MarketStatus
  ethMarket?: MarketStatus
  vaultJournal?: [boolean, boolean]
  coreJournal?: [boolean, boolean]
  issues: LiveDataIssue[]
}

export type LiveDataIssue = {
  source:
    | 'agent'
    | 'snapshot'
    | 'orders'
    | 'fills'
    | 'eligibility'
    | 'vaultCycles'
    | 'coreCycles'
    | 'btcMarket'
    | 'ethMarket'
    | 'vaultJournal'
    | 'coreJournal'
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
  private generation = 0

  get session(): SessionData | undefined {
    return this.active
  }

  async login(): Promise<SessionData> {
    const generation = ++this.generation
    const clients = await createClients()
    const address = await connectWallet()
    const session = await this.openLoginSession(clients, address, generation)
    const envelope = await EnvelopeClient.create()
    if (generation !== this.generation) {
      await this.sealedVault(
        'revoke_session',
        vaultPrivateCodec.session(session),
        vaultPrivateCodec.empty,
        { address, session, clients, envelope },
      ).catch(() => undefined)
      throw new Error('セッションは破棄されました')
    }
    this.active = { address, session, clients, envelope }
    return this.active
  }

  private async openLoginSession(
    clients: CanisterClients,
    address: string,
    generation: number,
  ): Promise<SessionHandle> {
    for (let attempt = 0; attempt < 4; attempt++) {
      if (generation !== this.generation) throw new Error('セッションは破棄されました')
      // open_session consumes its challenge before the journal write. A busy
      // journal therefore needs a fresh challenge and signature on retry.
      const challenge = unwrap(
        await clients.vault.issue_challenge({
          principal: clients.principal,
          origin: location.origin,
          network: clients.config.stage === 'testnet' ? { Testnet: null } : { Local: null },
          purpose: { Login: null },
          eoa_address: hexToBytes(address, 20),
        }),
      )
      const signature = await signTypedData(address, new Uint8Array(challenge.typed_data))
      if (generation !== this.generation) throw new Error('セッションは破棄されました')
      try {
        return unwrap(
          await clients.vault.open_session({
            challenge_id: challenge.challenge_id,
            eoa_signature: signature,
          }),
        )
      } catch (error) {
        if (
          !(error instanceof CanisterError) ||
          error.code !== 'JournalWriterBusy' ||
          attempt === 3
        )
          throw error
        await new Promise((resolve) => setTimeout(resolve, 150 * 2 ** attempt))
      }
    }
    throw new Error('セッションを開始できませんでした')
  }

  async logout(): Promise<void> {
    this.generation++
    const active = this.active
    this.active = undefined
    if (active)
      await this.sealedVault(
        'revoke_session',
        vaultPrivateCodec.session(active.session),
        vaultPrivateCodec.empty,
        active,
      ).catch(() => undefined)
  }

  private require(): SessionData {
    if (!this.active) throw new Error('MetaMaskでログインしてください')
    return this.active
  }

  private async sealed<T>(
    method:
      | 'get_account_snapshot'
      | 'list_orders'
      | 'list_fills'
      | 'cancel_order'
      | 'get_order_by_request',
    plaintext: Uint8Array,
    decode: (bytes: Uint8Array) => T,
  ): Promise<T> {
    const active = this.require()
    const serverKey = new Uint8Array(unwrap(await active.clients.core.get_hpke_public_key()))
    const id = newRequestId()
    const expiresAt = nowMs() + 60_000n
    const canister = Principal.fromText(active.clients.config.tradingCore)
    const aad = envelopeAad(
      active.clients.config.stage,
      canister.toUint8Array(),
      method,
      active.clients.principal.toUint8Array(),
      id,
      expiresAt,
    )
    const response = unwrap(
      await active.clients.core[method]({
        key_id: serverKey,
        network: active.clients.config.stage === 'testnet' ? { Testnet: null } : { Local: null },
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

  private async sealedVault<T>(
    method:
      | 'revoke_session'
      | 'approve_agent_generation'
      | 'request_allocation'
      | 'request_withdrawal'
      | 'provision_reserve_account'
      | 'prepare_trading_account'
      | 'request_recovery'
      | 'eligibility_signing_claims'
      | 'register_eligibility'
      | 'builder_fee_signing_claims'
      | 'register_builder_fee_mock_consent',
    plaintext: Uint8Array,
    decode: (bytes: Uint8Array) => T,
    active: SessionData = this.require(),
  ): Promise<T> {
    const serverKey = new Uint8Array(unwrap(await active.clients.vault.get_hpke_public_key()))
    const id = newRequestId()
    const expiresAt = nowMs() + 60_000n
    const canister = Principal.fromText(active.clients.config.fundsVault)
    const aad = envelopeAad(
      active.clients.config.stage,
      canister.toUint8Array(),
      method,
      active.clients.principal.toUint8Array(),
      id,
      expiresAt,
    )
    const response = unwrap(
      await active.clients.vault.private_call({
        key_id: serverKey,
        network: active.clients.config.stage === 'testnet' ? { Testnet: null } : { Local: null },
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

  private async sealedCoreWrite<T>(
    method:
      | 'submit_order'
      | 'close_position'
      | 'close_all'
      | 'request_agent_generation'
      | 'cancel_all',
    plaintext: Uint8Array,
    decode: (bytes: Uint8Array) => T,
  ): Promise<T> {
    const active = this.require()
    const serverKey = new Uint8Array(unwrap(await active.clients.core.get_hpke_public_key()))
    const id = newRequestId()
    const expiresAt = nowMs() + 60_000n
    const canister = Principal.fromText(active.clients.config.tradingCore)
    const aad = envelopeAad(
      active.clients.config.stage,
      canister.toUint8Array(),
      method,
      active.clients.principal.toUint8Array(),
      id,
      expiresAt,
    )
    const response = unwrap(
      await active.clients.core.private_call({
        key_id: serverKey,
        network: active.clients.config.stage === 'testnet' ? { Testnet: null } : { Local: null },
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
    const active = this.require()
    const { session, clients } = active
    const funds = unwrap(await clients.vault.get_fund_status(session))
    if (active !== this.active) throw new Error('セッションは破棄されました')
    const fundEvents = await this.listFundEvents()
    if (active !== this.active) throw new Error('セッションは破棄されました')
    const optional = await Promise.allSettled([
      clients.core.get_agent_status(session).then(unwrap),
      this.sealed('get_account_snapshot', codec.snapshotQuery(session), codec.snapshot),
      this.listOrders(),
      this.listFills(),
      clients.vault.eligibility_status(session).then(unwrap),
      clients.vault.get_cycles_status().then(unwrap),
      clients.core.get_cycles_status().then(unwrap),
      clients.core.get_market_status('BTC').then(unwrap),
      clients.core.get_market_status('ETH').then(unwrap),
      clients.vault.get_journal_send_status().then(unwrap),
      clients.core.get_journal_send_status().then(unwrap),
    ])
    const issues: LiveDataIssue[] = []
    for (const result of optional) {
      if (
        result.status === 'rejected' &&
        result.reason instanceof CanisterError &&
        ['SessionExpired', 'SessionRevoked', 'Unauthenticated'].includes(result.reason.code)
      )
        throw result.reason
    }
    return {
      funds,
      fundEvents,
      agent: optionalValue(optional[0], 'agent', issues),
      snapshot: optionalValue(optional[1], 'snapshot', issues),
      orders: optionalValue(optional[2], 'orders', issues),
      fills: optionalValue(optional[3], 'fills', issues),
      eligibility: optionalValue(optional[4], 'eligibility', issues),
      vaultCycles: optionalValue(optional[5], 'vaultCycles', issues),
      coreCycles: optionalValue(optional[6], 'coreCycles', issues),
      btcMarket: optionalValue(optional[7], 'btcMarket', issues),
      ethMarket: optionalValue(optional[8], 'ethMarket', issues),
      vaultJournal: optionalValue(optional[9], 'vaultJournal', issues),
      coreJournal: optionalValue(optional[10], 'coreJournal', issues),
      issues,
    }
  }

  async listOrders(cursor?: Uint8Array): Promise<Page<OrderSummary>> {
    const { session } = this.require()
    return this.sealed('list_orders', codec.listQuery(session, cursor), codec.orders)
  }

  async lookupOrder(id: Uint8Array): Promise<OrderSummary | undefined> {
    const { session } = this.require()
    return (
      await this.sealed(
        'get_order_by_request',
        codec.orderRequestQuery(session, id),
        codec.orderRequestStatus,
      )
    ).order[0]
  }

  async listFills(cursor?: Uint8Array): Promise<Page<Fill>> {
    const { session } = this.require()
    return this.sealed('list_fills', codec.listQuery(session, cursor), codec.fills)
  }

  async listFundEvents(cursor?: Uint8Array | number[]): Promise<FundEvents> {
    const { session, clients } = this.require()
    return unwrap(await clients.vault.list_fund_events(session, cursor ? [cursor] : [], 100))
  }

  async fundingInstructions() {
    const { session, clients } = this.require()
    await this.prepareTradingAccount()
    try {
      return unwrap(await clients.vault.get_funding_instructions(session))
    } catch (cause) {
      if (!(cause instanceof CanisterError) || cause.code !== 'NotAllowed') throw cause
      await this.sealedVault(
        'provision_reserve_account',
        vaultPrivateCodec.session(session),
        vaultPrivateCodec.account,
      )
      return unwrap(await clients.vault.get_funding_instructions(session))
    }
  }

  async prepareTradingAccount(): Promise<Uint8Array> {
    const { session } = this.require()
    return this.sealedVault(
      'prepare_trading_account',
      vaultPrivateCodec.session(session),
      vaultPrivateCodec.account,
    )
  }

  async eligibilitySigningClaims(): Promise<Uint8Array> {
    const { session } = this.require()
    await this.prepareTradingAccount()
    return this.sealedVault(
      'eligibility_signing_claims',
      vaultPrivateCodec.eligibilitySigningQuery(session, nowMs() + 24n * 60n * 60n * 1_000n),
      vaultPrivateCodec.eligibilityClaims,
    )
  }

  async registerEligibility(claimsCandidHex: string, signatureHex: string) {
    const { session } = this.require()
    return this.sealedVault(
      'register_eligibility',
      vaultPrivateCodec.eligibilityRegister(
        session,
        hexToBytes(claimsCandidHex),
        hexToBytes(signatureHex, 65),
      ),
      vaultPrivateCodec.eligibilityStatus,
    )
  }

  async approveZeroBuilderFeeMock(builderAddress: string) {
    const { session, address } = this.require()
    const target = await this.sealedVault(
      'builder_fee_signing_claims',
      vaultPrivateCodec.builderFeeSigningQuery(
        session,
        hexToBytes(builderAddress, 20),
        nowMs() + 24n * 60n * 60n * 1_000n,
      ),
      vaultPrivateCodec.builderFeeTarget,
    )
    const signature = await signPersonalBytes(address, new Uint8Array(target[1]))
    return this.sealedVault(
      'register_builder_fee_mock_consent',
      vaultPrivateCodec.builderFeeRegister(session, target[0], signature),
      vaultPrivateCodec.builderFeeStatus,
    )
  }

  async seedDeposit(amount: string): Promise<void> {
    const active = this.require()
    if (active.clients.config.stage !== 'local') throw new Error('mock 入金はローカル専用です')
    const instructions = await this.fundingInstructions()
    const response = await fetch(`${active.clients.config.mockHl}/admin/deposits`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        address: `0x${[...instructions.hl_account_address].map((value) => value.toString(16).padStart(2, '0')).join('')}`,
        amount,
        sender: active.address,
        id: crypto.randomUUID(),
      }),
    })
    if (!response.ok) throw new Error(`LOCAL MOCK seed失敗: ${response.status}`)
  }

  async allocate(amount: bigint) {
    const { session } = this.require()
    return this.sealedVault(
      'request_allocation',
      vaultPrivateCodec.allocation(session, requestId(), amount),
      vaultPrivateCodec.fund,
    )
  }

  async depositToReserve(amount: string, value: bigint) {
    const active = this.require()
    const read = async () => {
      if (this.active !== active) throw new Error('セッションが変更されました。')
      const status = unwrap(await active.clients.vault.get_fund_status(active.session))
      if (this.active !== active) throw new Error('セッションが変更されました。')
      return status
    }
    const before = await read()
    if (before.unknowns.length || before.recovery_fence.length)
      throw new Error('先の資金移動を照合中です。履歴を確認してください。')
    await this.seedDeposit(amount)
    await waitForFunds(read, (status) => status.withdrawable >= before.withdrawable + value)
  }

  async allocateFromReserve(value: bigint) {
    const active = this.require()
    const read = async () => {
      if (this.active !== active) throw new Error('セッションが変更されました。')
      const status = unwrap(await active.clients.vault.get_fund_status(active.session))
      if (this.active !== active) throw new Error('セッションが変更されました。')
      return status
    }
    const before = await read()
    if (before.unknowns.length || before.recovery_fence.length)
      throw new Error('先の資金移動を照合中です。履歴を確認してください。')
    const accepted = await this.allocate(value)
    await waitForFunds(
      read,
      (status) =>
        status.in_transit === 0n && status.trading_equity >= before.trading_equity + value,
    )
    return accepted
  }

  async recover(amount: bigint) {
    const { session } = this.require()
    return this.sealedVault(
      'request_recovery',
      vaultPrivateCodec.recovery(session, requestId(), amount),
      vaultPrivateCodec.fund,
    )
  }

  async approveAgent() {
    const { session } = this.require()
    const generation = await this.sealedCoreWrite(
      'request_agent_generation',
      corePrivateCodec.session(session),
      corePrivateCodec.agent,
    )
    return this.sealedVault(
      'approve_agent_generation',
      vaultPrivateCodec.approveAgent(session, generation.generation, generation.agent_address),
      vaultPrivateCodec.agent,
    )
  }

  async submitOrder(args: {
    market: string
    side: 'buy' | 'sell'
    kind: 'market' | 'limit'
    quantity: string
    price: string
    leverage?: number
    slippageBps?: number
    reduceOnly?: boolean
    trigger?: { kind: 'stopLoss' | 'takeProfit'; price: string; isMarket: boolean }
    clientRequestId?: Uint8Array
  }): Promise<SubmitOrderResult> {
    const active = this.require()
    const { session, clients } = active
    let account: Uint8Array | number[] | undefined
    try {
      account = unwrap(await clients.vault.get_trading_account(session))[0]
      if (!account) throw new Error('取引口座がありません。先に配分してください')
      if (active !== this.active) throw new Error('セッションは破棄されました')
    } catch (cause) {
      if (
        cause instanceof CanisterError &&
        ['SessionExpired', 'SessionRevoked', 'Unauthenticated'].includes(cause.code)
      )
        throw cause
      throw new SubmissionNotSentError(errorMessage(cause))
    }
    return this.sealedCoreWrite(
      'submit_order',
      corePrivateCodec.submit(session, {
        session,
        account_id: account,
        client_request_id: args.clientRequestId ?? requestId(),
        market: args.market,
        side: args.side === 'buy' ? { Buy: null } : { Sell: null },
        kind: args.kind === 'market' ? { MarketIoc: null } : { LimitGtc: null },
        quantity: args.quantity,
        limit_price: [args.price],
        leverage: [args.leverage ?? 3],
        slippage_tolerance_bps: args.kind === 'market' ? [args.slippageBps ?? 50] : [],
        reduce_only: args.reduceOnly ?? false,
        trigger: args.trigger
          ? [
              {
                kind: args.trigger.kind === 'stopLoss' ? { StopLoss: null } : { TakeProfit: null },
                is_market: args.trigger.isMarket,
                trigger_price: args.trigger.price,
              },
            ]
          : [],
        expires_after: [nowMs() + 60_000n],
      }),
      corePrivateCodec.submitResult,
    )
  }

  async cancel(orderId: Uint8Array | number[]): Promise<void> {
    const { session } = this.require()
    await this.sealed('cancel_order', codec.cancelQuery(session, orderId), codec.empty)
  }

  async cancelAll(): Promise<bigint> {
    const { session } = this.require()
    return this.sealedCoreWrite(
      'cancel_all',
      corePrivateCodec.session(session),
      corePrivateCodec.count,
    )
  }

  async closePosition(position: Position, ratioBps: number): Promise<SubmitOrderResult> {
    const { session } = this.require()
    return this.sealedCoreWrite(
      'close_position',
      corePrivateCodec.closePosition(session, requestId(), position.market, ratioBps, []),
      corePrivateCodec.submitResult,
    )
  }

  async closeAll() {
    const { session } = this.require()
    return this.sealedCoreWrite(
      'close_all',
      corePrivateCodec.closeAll(session, requestId()),
      corePrivateCodec.closeOutcome,
    )
  }

  async protectPosition(
    position: Position,
    kind: 'stopLoss' | 'takeProfit',
    price: string,
  ): Promise<SubmitOrderResult> {
    const isLong = !position.size.startsWith('-')
    return this.submitOrder({
      market: position.market,
      side: isLong ? 'sell' : 'buy',
      kind: 'limit',
      quantity: position.size.replace('-', ''),
      price,
      reduceOnly: true,
      trigger: { kind, price, isMarket: true },
    })
  }

  async withdraw(amount: bigint) {
    const active = this.require()
    const { address, session, clients } = active
    const read = async () => {
      if (this.active !== active) throw new Error('セッションが変更されました。')
      const status = unwrap(await clients.vault.get_fund_status(session))
      if (this.active !== active) throw new Error('セッションが変更されました。')
      return status
    }
    await prepareWithdrawal(amount, read, (shortfall) => this.recover(shortfall))
    if (this.active !== active) throw new Error('セッションが変更されました。')
    const nonce = nowMs()
    const expiresAt = nonce + 300_000n
    const typedData = withdrawalTypedData({
      address,
      amount,
      nonce,
      expiresAt,
      canister: Principal.fromText(clients.config.fundsVault),
      network: clients.config.stage,
    })
    const signature = await signTypedData(address, typedData)
    if (this.active !== active) throw new Error('セッションが変更されました。')
    return this.sealedVault(
      'request_withdrawal',
      vaultPrivateCodec.withdrawal({
        session,
        client_request_id: requestId(),
        amount,
        asset: { Usdc: null },
        destination: { AuthenticatedEoaHlAccount: null },
        network: clients.config.stage === 'testnet' ? { Testnet: null } : { Local: null },
        nonce,
        expires_at: expiresAt,
        intent_signature: signature,
      }),
      vaultPrivateCodec.fund,
    )
  }
}
