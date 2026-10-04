// Explicit opt-in: local IC canisters and real HL testnet.
// Default smoke performs no exchange POST; TESTNET_ACCEPTANCE enables live transfers.
import { execFileSync } from 'node:child_process'
import { writeFileSync, existsSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { expect, test } from 'vitest'
import { Principal } from '@icp-sdk/core/principal'
import { createClients } from '../../src/client/ic'
import { codec, corePrivateCodec, vaultPrivateCodec } from '../../src/client/candid-codec'
import { EnvelopeClient, envelopeAad, newRequestId } from '../../src/client/envelope'
import { unwrap } from '../../src/client/result'
import { bytesToHex, hexToBytes, withdrawalTypedData } from '../../src/client/wallet'

test.skipIf(process.env.TESTNET_SMOKE !== '1')(
  'prepare local custody accounts on HL testnet',
  async () => {
    if (!process.env.PRIVATE_PERP_TESTNET_EOA_KEY) throw new Error('testnet owner key required')
    const appId = process.env.TESTNET_APP_ID
    if (!appId) throw new Error('TESTNET_APP_ID is required')
    const clients = await createClients({
      stage: 'local',
      privatePerp: appId,
      host: 'http://127.0.0.1:18100',
      fundsVault: appId,
      tradingCore: appId,
      mockHl: 'http://127.0.0.1:8080',
      marketWs: 'ws://127.0.0.1:8080/ws',
      fetchRootKey: true,
    })
    expect(unwrap(await clients.vault.get_environment()).network).toEqual({ Testnet: null })
    const signer = resolve('../target/debug/e2e-signer')
    const address = execFileSync(signer, ['address'], { encoding: 'utf8' }).trim()
    const challenge = unwrap(
      await clients.vault.issue_challenge({
        principal: clients.principal,
        origin: 'http://127.0.0.1:18100',
        network: { Testnet: null },
        purpose: { Login: null },
        eoa_address: hexToBytes(address, 20),
      }),
    )
    const signature = execFileSync(signer, [], {
      input: Buffer.from(challenge.typed_data),
      encoding: 'utf8',
    }).trim()
    const session = unwrap(
      await clients.vault.open_session({
        challenge_id: challenge.challenge_id,
        eoa_signature: hexToBytes(signature, 65),
      }),
    )
    if (process.env.TESTNET_PREFLIGHT_ONLY === '1') {
      const funds = unwrap(await clients.vault.get_fund_status(session))
      const requests = unwrap(await clients.vault.list_fund_events(session, [], 100))
      expect(funds.unknowns).toHaveLength(0)
      expect(funds.recovery_fence).toHaveLength(0)
      expect(funds.in_transit).toBe(0n)
      expect(funds.reserved_for_withdrawal).toBe(0n)
      expect(requests.next_cursor).toHaveLength(0)
      expect(
        requests.items.every((event) => 'Settled' in event.state || 'Rejected' in event.state),
      ).toBe(true)
      writeFileSync(
        resolve('../.icp-home/hl-testnet/reset-ledger-preflight.json'),
        JSON.stringify(
          {
            appId,
            ownerAddress: address,
            observedAt: new Date().toISOString(),
            funds,
            requestStates: requests.items.map((event) => Object.keys(event.state)[0]),
          },
          (_, value) => (typeof value === 'bigint' ? value.toString() : value),
          2,
        ) + '\n',
        { mode: 0o600 },
      )
      return
    }
    const envelope = await EnvelopeClient.create()
    const canister = Principal.fromText(appId)
    async function sealed<T>(
      method: string,
      plaintext: Uint8Array,
      decode: (bytes: Uint8Array) => T,
    ) {
      const serverKey = new Uint8Array(unwrap(await clients.vault.get_hpke_public_key()))
      const id = newRequestId()
      const expiresAt = BigInt(Date.now()) + 60_000n
      const aad = envelopeAad(
        'testnet',
        canister.toUint8Array(),
        method,
        clients.principal.toUint8Array(),
        id,
        expiresAt,
      )
      const response = unwrap(
        await clients.vault.private_call({
          key_id: serverKey,
          network: { Testnet: null },
          canister,
          method,
          request_id: id,
          expires_at: expiresAt,
          client_public_key: envelope.publicKey,
          aad,
          ciphertext: await envelope.seal(serverKey, aad, plaintext),
        }),
      )
      expect(Array.from(response.request_id)).toEqual(Array.from(id))
      return decode(await envelope.open(aad, new Uint8Array(response.ciphertext)))
    }
    const tradingId = await sealed(
      'prepare_trading_account',
      vaultPrivateCodec.session(session),
      vaultPrivateCodec.account,
    )
    const claims = await sealed(
      'eligibility_signing_claims',
      vaultPrivateCodec.eligibilitySigningQuery(session, BigInt(Date.now()) + 86_400_000n),
      vaultPrivateCodec.eligibilityClaims,
    )
    const eligibilitySignature = execFileSync(resolve('../target/debug/eligibility-issuer'), [], {
      input: bytesToHex(claims),
      encoding: 'utf8',
    }).trim()
    const eligibility = await sealed(
      'register_eligibility',
      vaultPrivateCodec.eligibilityRegister(session, claims, hexToBytes(eligibilitySignature, 65)),
      vaultPrivateCodec.eligibilityStatus,
    )
    expect(eligibility.eligible).toBe(true)
    await sealed(
      'provision_reserve_account',
      vaultPrivateCodec.session(session),
      vaultPrivateCodec.account,
    )
    const funding = unwrap(await clients.vault.get_funding_instructions(session))
    expect(funding.network).toEqual({ Testnet: null })
    const reserveAddress = bytesToHex(Array.from(funding.hl_account_address))
    const result = await fetch('https://api.hyperliquid-testnet.xyz/info', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ type: 'clearinghouseState', user: reserveAddress }),
    })
    expect(result.ok).toBe(true)
    const state = (await result.json()) as { marginSummary: { accountValue: string } }
    expect(typeof state.marginSummary.accountValue).toBe('string')
    const tradingAddress = bytesToHex(unwrap(await clients.vault.get_trading_address(session)))
    const initialFunds = unwrap(await clients.vault.get_fund_status(session))
    const initialEvents = unwrap(await clients.vault.list_fund_events(session, [], 100))
    const pendingEvents = initialEvents.items.filter(
      (event) => !('Settled' in event.state) && !('Rejected' in event.state),
    )
    if (
      process.env.TESTNET_REQUIRE_SETTLED === '1' ||
      process.env.TESTNET_REQUIRE_EMPTY_LEDGER === '1'
    ) {
      expect(initialFunds.unknowns).toHaveLength(0)
      expect(initialFunds.recovery_fence).toHaveLength(0)
      expect(initialFunds.in_transit).toBe(0n)
      expect(initialFunds.reserved_for_withdrawal).toBe(0n)
      expect(pendingEvents).toHaveLength(0)
      expect(initialEvents.next_cursor).toHaveLength(0)
    }
    if (process.env.TESTNET_REQUIRE_EMPTY_LEDGER === '1') {
      expect(initialFunds.reserve_unallocated).toBe(0n)
      expect(initialFunds.trading_equity).toBe(0n)
      expect(initialFunds.withdrawable).toBe(0n)
      expect(initialEvents.items).toHaveLength(0)
      expect(Number(state.marginSummary.accountValue)).toBe(0)
    }
    const report = {
      icHost: clients.config.host,
      hlNetwork: 'testnet',
      ownerAddress: address,
      reserveAddress,
      tradingAccountId: bytesToHex(tradingId),
      tradingAddress,
      ledger: {
        reserveUnallocated: initialFunds.reserve_unallocated.toString(),
        tradingEquity: initialFunds.trading_equity.toString(),
        inTransit: initialFunds.in_transit.toString(),
        reservedForWithdrawal: initialFunds.reserved_for_withdrawal.toString(),
        withdrawable: initialFunds.withdrawable.toString(),
        unknowns: initialFunds.unknowns.length,
        recoveryFences: initialFunds.recovery_fence.length,
        fundRequests: initialEvents.items.length,
        pendingFundRequests: pendingEvents.length,
      },
      accountValue: state.marginSummary.accountValue,
      observedAt: new Date().toISOString(),
      exchangePostSent: false,
    }
    writeFileSync(
      resolve('../.icp-home/hl-testnet/public.json'),
      JSON.stringify(report, null, 2) + '\n',
    )
    console.log(JSON.stringify(report, null, 2))
    if (process.env.TESTNET_ACCEPTANCE !== '1') return
    const events: unknown[] = []
    const save = (step: string, details: unknown) => {
      events.push({ step, at: new Date().toISOString(), details })
      writeFileSync(
        resolve('../.icp-home/hl-testnet/acceptance-' + appId + '.json'),
        JSON.stringify(
          { appId, ownerAddress: address, reserveAddress, events },
          (_, value) => (typeof value === 'bigint' ? value.toString() : value),
          2,
        ) + '\n',
        { mode: 0o600 },
      )
      console.log(step, details)
    }
    async function coreSealed<T>(
      method: string,
      plaintext: Uint8Array,
      decode: (bytes: Uint8Array) => T,
    ) {
      const key = new Uint8Array(unwrap(await clients.core.get_hpke_public_key()))
      const id = newRequestId()
      const expiresAt = BigInt(Date.now()) + 60_000n
      const aad = envelopeAad(
        'testnet',
        canister.toUint8Array(),
        method,
        clients.principal.toUint8Array(),
        id,
        expiresAt,
      )
      const request = {
        key_id: key,
        network: { Testnet: null },
        canister,
        method,
        request_id: id,
        expires_at: expiresAt,
        client_public_key: envelope.publicKey,
        aad,
        ciphertext: await envelope.seal(key, aad, plaintext),
      }
      const response = unwrap(
        await (method === 'list_orders'
          ? clients.core.list_orders(request)
          : method === 'cancel_order'
            ? clients.core.cancel_order(request)
            : clients.core.private_call(request)),
      )
      return decode(await envelope.open(aad, new Uint8Array(response.ciphertext)))
    }
    const adminCall = (method: string, args = '()') => {
      const output = execFileSync('icp', ['canister', 'call', 'private_perp', method, args], {
        cwd: resolve('..'),
        env: { ...process.env, ICP_HOME: resolve('../.icp-home') },
        encoding: 'utf8',
      })
      if (output.includes('Err =')) throw new Error(output)
      return output
    }
    const wait = async <T>(read: () => Promise<T>, done: (value: T) => boolean): Promise<T> => {
      const end = Date.now() + 90_000
      while (true) {
        const value = await read()
        if (done(value)) return value
        if (Date.now() > end)
          throw new Error(
            'Read-only reconciliation timed out: ' +
              JSON.stringify(value, (_, v) => (typeof v === 'bigint' ? v.toString() : v)),
          )
        await new Promise((resolve) => setTimeout(resolve, 2000))
      }
    }
    save('prepared', report)
    const depositRecord = resolve('../.icp-home/hl-testnet/deposit-' + appId + '.json')
    if (!existsSync(depositRecord)) {
      const deposit = execFileSync(process.execPath, ['testnet-deposit.mjs'], {
        cwd: resolve('../tools/hl-fixture-gen'),
        encoding: 'utf8',
      })
      save('deposit-response', JSON.parse(deposit.trim()))
    } else {
      const previous = JSON.parse(readFileSync(depositRecord, 'utf8'))
      expect(previous.destination.toLowerCase()).toBe(reserveAddress.toLowerCase())
      expect(previous.response?.status).toBe('ok')
      save('deposit-already-sent', {
        destination: previous.destination,
        response: previous.response,
      })
    }
    const credited = await wait(
      async () => {
        adminCall(
          'reconcile_deposits',
          '(vec {' +
            Array.from(funding.hl_account_address)
              .map((x) => x + ' : nat8')
              .join(';') +
            '})',
        )
        return unwrap(await clients.vault.get_fund_status(session))
      },
      (status) => status.reserve_unallocated + status.trading_equity >= 8_000_000n,
    )
    save('deposit-credited', credited)
    if (credited.trading_equity < 4_000_000n) {
      const allocated = await sealed(
        'request_allocation',
        vaultPrivateCodec.allocation(session, newRequestId(), 5_000_000n),
        vaultPrivateCodec.fund,
      )
      save('allocation-request', allocated)
    }
    const funded = await wait(
      () => clients.vault.get_fund_status(session).then(unwrap),
      (s) => s.trading_equity >= 4_000_000n && s.in_transit === 0n,
    )
    save('allocation-settled', funded)
    const priorAgent = unwrap(await clients.core.get_agent_status(session))
    if (!priorAgent.current[0] || !('Active' in priorAgent.current[0].state)) {
      const agent = await coreSealed(
        'request_agent_generation',
        corePrivateCodec.session(session),
        corePrivateCodec.agent,
      )
      save('agent-requested', agent)
      await sealed(
        'approve_agent_generation',
        vaultPrivateCodec.approveAgent(session, agent.generation, agent.agent_address),
        vaultPrivateCodec.agent,
      )
    }
    const active = await wait(
      () => clients.core.get_agent_status(session).then(unwrap),
      (s) => s.current[0] !== undefined && 'Active' in s.current[0].state,
    )
    save('agent-approved', active)
    const meta = (await fetch('https://api.hyperliquid-testnet.xyz/info', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ type: 'metaAndAssetCtxs' }),
    }).then((r) => r.json())) as [
      { universe: { name: string; szDecimals: number }[] },
      { markPx: string }[],
    ]
    const index = meta[0].universe.findIndex((m: { name: string }) => m.name === 'ETH')
    const price = Math.floor(Number(meta[1][index].markPx) * 0.7)
    const factor = 10 ** meta[0].universe[index].szDecimals
    const quantity = (Math.ceil((11 / price) * factor) / factor).toString()
    expect(price).toBeGreaterThan(0)
    expect(Number(quantity) * price).toBeLessThan(14)
    adminCall(
      'core_configure_market_threshold',
      '(record { market="ETH"; expected_index=' +
        index +
        ' : nat32; min_day_notional_usdc=250000 : nat64; max_spread_bps=20 : nat32; min_each_side_depth_usdc=1000 : nat64 })',
    )
    adminCall('refresh_market')
    adminCall('sweep')
    if (process.env.TESTNET_FUNDS_ONLY === '1') {
      save('order-not-tested', {
        reason: 'Insufficient equity for venue minimum notional',
        equity: funded.trading_equity,
      })
    } else {
      const order = await coreSealed(
        'submit_order',
        corePrivateCodec.submit(session, {
          session,
          client_request_id: newRequestId(),
          account_id: tradingId,
          market: 'ETH',
          side: { Buy: null },
          kind: { LimitGtc: null },
          quantity,
          limit_price: [price.toString()],
          slippage_tolerance_bps: [],
          reduce_only: false,
          leverage: [3],
          trigger: [],
          expires_after: [],
        }),
        corePrivateCodec.submitResult,
      )
      save('order-submitted', order)
      adminCall('sweep')
      const listed = await wait(
        () => coreSealed('list_orders', codec.listQuery(session), codec.orders),
        (page) =>
          page.items.some(
            (o) => bytesToHex(o.order_id) === bytesToHex(order.order_id) && 'Open' in o.state,
          ),
      )
      save('order-open', listed)
      await coreSealed('cancel_order', codec.cancelQuery(session, order.order_id), codec.empty)
      adminCall('sweep')
      const cancelled = await wait(
        () => coreSealed('list_orders', codec.listQuery(session), codec.orders),
        (page) =>
          page.items.some(
            (o) => bytesToHex(o.order_id) === bytesToHex(order.order_id) && 'Cancelled' in o.state,
          ),
      )
      save('order-cancelled', cancelled)
    }
    const recovery = await sealed(
      'request_recovery',
      vaultPrivateCodec.recovery(session, newRequestId(), 4_000_000n),
      vaultPrivateCodec.fund,
    )
    save('recovery-request', recovery)
    const recovered = await wait(
      () => clients.vault.get_fund_status(session).then(unwrap),
      (s) =>
        s.in_transit === 0n && s.recovery_fence.length === 0 && s.reserve_unallocated >= 8_000_000n,
    )
    save('recovery-settled', recovered)
    const reserveState = (await fetch('https://api.hyperliquid-testnet.xyz/info', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ type: 'clearinghouseState', user: reserveAddress }),
    }).then((r) => r.json())) as { withdrawable: string }
    const amount = BigInt(Math.floor(Number(reserveState.withdrawable) * 1_000_000))
    expect(amount).toBeGreaterThan(0n)
    expect(amount).toBeLessThanOrEqual(8_000_000n)
    save('actual-reserve-balance', { amount, recorded: recovered.reserve_unallocated })
    const nonce = BigInt(Date.now()),
      expiresAt = nonce + 300_000n
    const withdrawalSignature = execFileSync(signer, [], {
      input: withdrawalTypedData({
        address,
        amount,
        nonce,
        expiresAt,
        canister,
        network: 'testnet',
      }),
      encoding: 'utf8',
    }).trim()
    const withdrawal = await sealed(
      'request_withdrawal',
      vaultPrivateCodec.withdrawal({
        session,
        client_request_id: newRequestId(),
        amount,
        asset: { Usdc: null },
        destination: { AuthenticatedEoaHlAccount: null },
        network: { Testnet: null },
        nonce,
        expires_at: expiresAt,
        intent_signature: hexToBytes(withdrawalSignature, 65),
      }),
      vaultPrivateCodec.fund,
    )
    save('withdrawal-request', withdrawal)
    const withdrawn = await wait(
      () => clients.vault.get_fund_status(session).then(unwrap),
      (s) =>
        s.in_transit === 0n &&
        s.reserved_for_withdrawal === 0n &&
        s.reserve_unallocated === recovered.reserve_unallocated - amount,
    )
    save('withdrawal-settled', withdrawn)
  },
)
