// Explicit opt-in: local IC canisters, real HL testnet, no mock seed or exchange POST.
import { execFileSync } from 'node:child_process'
import { writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { expect, test } from 'vitest'
import { Principal } from '@icp-sdk/core/principal'
import { createClients } from '../../src/client/ic'
import { vaultPrivateCodec } from '../../src/client/candid-codec'
import { EnvelopeClient, envelopeAad, newRequestId } from '../../src/client/envelope'
import { unwrap } from '../../src/client/result'
import { bytesToHex, hexToBytes } from '../../src/client/wallet'

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
    const report = {
      icHost: clients.config.host,
      hlNetwork: 'testnet',
      ownerAddress: address,
      reserveAddress,
      tradingAccountId: bytesToHex(tradingId),
      accountValue: state.marginSummary.accountValue,
      observedAt: new Date().toISOString(),
      exchangePostSent: false,
    }
    writeFileSync(
      resolve('../.icp-home/hl-testnet/public.json'),
      JSON.stringify(report, null, 2) + '\n',
    )
    console.log(JSON.stringify(report, null, 2))
  },
)
