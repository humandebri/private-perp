// canister IDと接続先の解決（ハードコードしない）。
//
// 画面は`VITE_*`の環境変数から読む。ローカルのIDはデプロイごとに変わり得るため、
// `.env.local`（gitignore対象）で上書きし、`.env.example`に例を置く。

export type ClientConfig = {
  /** ICのホスト（ローカルは`http://127.0.0.1:18100`）。 */
  host: string
  fundsVault: string
  tradingCore: string
  /** ローカル専用Hyperliquid mock。 */
  mockHl: string
  /** ローカルネットワークではroot keyを取得する（検証済みqueryのため）。 */
  fetchRootKey: boolean
}

type Env = Record<string, string | boolean | undefined>

function required(env: Env, name: string): string {
  const value = env[name]
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error(
      `${name} が設定されていません（frontend/.env.local、例は frontend/.env.example）`,
    )
  }
  return value
}

/** 環境変数から設定を読む（未設定は例外。黙って既定のcanisterへ繋がない）。 */
export function resolveConfig(env: Env = import.meta.env as unknown as Env): ClientConfig {
  if (env.VITE_APP_STAGE !== 'local') throw new Error('VITE_APP_STAGE=local が必要です')
  const host = required(env, 'VITE_IC_HOST')
  const mockHl = required(env, 'VITE_MOCK_HL_URL')
  for (const [name, value] of [
    ['VITE_IC_HOST', host],
    ['VITE_MOCK_HL_URL', mockHl],
  ]) {
    const hostname = new URL(value).hostname
    if (!['127.0.0.1', 'localhost', '::1'].includes(hostname)) {
      throw new Error(`${name} はloopbackのみ指定できます`)
    }
  }
  return {
    host,
    fundsVault: required(env, 'VITE_FUNDS_VAULT_CANISTER_ID'),
    tradingCore: required(env, 'VITE_TRADING_CORE_CANISTER_ID'),
    mockHl,
    // ローカル（httpsでない）ホストではroot keyを取得する。
    fetchRootKey: host.startsWith('http://'),
  }
}
