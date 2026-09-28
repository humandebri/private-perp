// canister IDと接続先の解決（ハードコードしない）。
//
// 画面は`VITE_*`の環境変数から読む。ローカルのIDはデプロイごとに変わり得るため、
// `.env.local`（gitignore対象）で上書きし、`.env.example`に例を置く。

export type ClientConfig = {
  stage: 'local' | 'testnet'
  /** ICのホスト（ローカルは`http://127.0.0.1:18100`）。 */
  host: string
  fundsVault: string
  tradingCore: string
  privatePerp: string
  /** ローカル専用Hyperliquid mock。 */
  mockHl: string
  /** 公開市況専用のローカルWebSocket。本人識別子は送らない。 */
  marketWs: string
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
  const stage = env.VITE_APP_STAGE
  if (stage !== 'local' && stage !== 'testnet')
    throw new Error('VITE_APP_STAGE は local または testnet が必要です')
  const host = required(env, 'VITE_IC_HOST')
  const mockHl = stage === 'local' ? required(env, 'VITE_MOCK_HL_URL') : ''
  const marketWs = required(env, 'VITE_MARKET_WS_URL')
  if (stage === 'local') {
    for (const [name, value] of [
      ['VITE_IC_HOST', host],
      ['VITE_MOCK_HL_URL', mockHl],
      ['VITE_MARKET_WS_URL', marketWs],
    ]) {
      const hostname = new URL(value).hostname
      if (!['127.0.0.1', 'localhost', '::1'].includes(hostname)) {
        throw new Error(`${name} はloopbackのみ指定できます`)
      }
    }
  } else if (new URL(host).protocol !== 'https:' || new URL(marketWs).protocol !== 'wss:') {
    throw new Error('testnet では HTTPS と WSS が必要です')
  }
  if (!['ws:', 'wss:'].includes(new URL(marketWs).protocol)) {
    throw new Error('VITE_MARKET_WS_URL はWebSocket URLである必要があります')
  }
  return {
    stage,
    host,
    fundsVault: required(env, 'VITE_PRIVATE_PERP_CANISTER_ID'),
    tradingCore: required(env, 'VITE_PRIVATE_PERP_CANISTER_ID'),
    privatePerp: required(env, 'VITE_PRIVATE_PERP_CANISTER_ID'),
    mockHl,
    marketWs,
    // ローカル（httpsでない）ホストではroot keyを取得する。
    fetchRootKey: host.startsWith('http://'),
  }
}
