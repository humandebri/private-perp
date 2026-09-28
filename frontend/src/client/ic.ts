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
import { idlFactory as privatePerpIdl } from './candid/private_perp.did.js'
import type { _SERVICE as FundsVaultService } from './candid/funds_vault.did.js'
import type { _SERVICE as TradingCoreService } from './candid/trading_core.did.js'
import type { _SERVICE as PrivatePerpService } from './candid/private_perp.did.js'

const sharedMethodNames = new Set([
  'configure_cycles',
  'configure_eligibility',
  'configure_market_threshold',
  'configure_rest_budget',
  'get_cycles_status',
  'get_environment',
  'get_hpke_public_key',
  'get_journal_guard',
  'get_journal_send_status',
  'get_policy_principal',
  'get_send_journal',
  'journal_restore_status',
  'private_call',
  'recovery_replay_pending',
  'recovery_stage_status',
  'resume_journal',
  'rotate_hpke_key',
  'set_ecdsa_key_id',
  'set_journal_guard',
  'set_policy_principal',
  'set_send_journal',
  'set_sns_principal',
  'set_venue_endpoints',
  'test_sweep_now',
  'version',
])

function roleView<T>(actor: PrivatePerpService, role: 'vault' | 'core'): T {
  return new Proxy(actor, {
    get(target, property) {
      const name =
        typeof property === 'string' && sharedMethodNames.has(property)
          ? `${role}_${property}`
          : property
      return Reflect.get(target, name)
    },
  }) as T
}

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
  const combined = Actor.createActor<PrivatePerpService>(privatePerpIdl, {
    agent,
    canisterId: config.privatePerp,
  })
  return {
    identity,
    principal: identity.getPrincipal(),
    agent,
    vault: roleView<FundsVaultService>(combined, 'vault'),
    core: roleView<TradingCoreService>(combined, 'core'),
    config,
  }
}
