// IC接続層。セッション中だけ持つ短命のEd25519 identityでcanisterを呼ぶ。
//
// `docs/phase-0/api-contract.md` 2.1のとおり、challengeは「ブラウザが生成する
// 短命IC署名IdentityのPrincipal」へ束縛する。identityはメモリのみに置き、
// `localStorage`・`IndexedDB`・Cookieへ保存しない（同6節）。

import { Actor, HttpAgent } from '@icp-sdk/core/agent'
import { Ed25519KeyIdentity } from '@icp-sdk/core/identity'
import { Principal } from '@icp-sdk/core/principal'

import { resolveConfig } from './config'
import type { ClientConfig } from './config'
import { idlFactory as fundsVaultIdl } from './candid/funds_vault.did.js'
import { idlFactory as tradingCoreIdl } from './candid/trading_core.did.js'
import type { _SERVICE as FundsVaultService } from './candid/funds_vault.did.js'
import type { _SERVICE as TradingCoreService } from './candid/trading_core.did.js'

/** 短命のIC identity（クライアント鍵。ページを閉じると失われる）。 */
export function createSessionIdentity(): Ed25519KeyIdentity {
  return Ed25519KeyIdentity.generate()
}

/** agentを作る。ローカル（http）ではroot keyを取得する。 */
export async function createAgent(
  identity: Ed25519KeyIdentity,
  config: ClientConfig = resolveConfig(),
): Promise<HttpAgent> {
  // 書込結果が不明なときは照合する。transportによる自動再送も行わない。
  const agent = HttpAgent.createSync({ host: config.host, identity, retryTimes: 0 })
  if (config.fetchRootKey) await agent.fetchRootKey()
  return agent
}

export type CanisterClients = {
  identity: Ed25519KeyIdentity
  principal: Principal
  agent: HttpAgent
  vault: FundsVaultService
  core: TradingCoreService
  config: ClientConfig
}

/** 新しいsession identityで両canisterのactorを作る。 */
export async function createClients(
  config: ClientConfig = resolveConfig(),
): Promise<CanisterClients> {
  const identity = createSessionIdentity()
  const agent = await createAgent(identity, config)
  return {
    identity,
    principal: identity.getPrincipal(),
    agent,
    vault: Actor.createActor<FundsVaultService>(fundsVaultIdl, {
      agent,
      canisterId: config.fundsVault,
    }),
    core: Actor.createActor<TradingCoreService>(tradingCoreIdl, {
      agent,
      canisterId: config.tradingCore,
    }),
    config,
  }
}
