# private-perp UI

TanStack Start＋Reactで、ローカルICP replicaとローカルHyperliquid mockへ実接続する単一ユーザー開発UIです。公開市況WebSocket、建玉、SL/TP、決済、取消と最小クライアントを含みます。testnet・mainnet・実資金には接続しません。

## 一括起動・E2E

リポジトリルートで次を実行します。

```sh
bash scripts/local-e2e.sh
```

mock HL起動、ローカルIC network、4 Canisterのdeploy/bootstrap、短命の`frontend/.env.local`生成、frontend build、Playwrightを一括実行し、終了時に子プロセスとIC networkを停止します。Playwrightは固定秘密鍵を製品コードへ入れず、試験専用`e2e-signer`をEIP-1193 providerとして注入します。

手動起動では`.env.example`を`.env.local`へ写し、deploy後の2 Canister IDを設定してください。`VITE_APP_STAGE=local`、IC host、mock HTTP/WS URLの全てが必須で、loopback以外は拒否します。mockの管理APIを別のfrontendポートから使う場合は、起動時の`MOCK_HL_ADMIN_ORIGINS`へ完全なOriginをカンマ区切りで指定します（既定は`127.0.0.1:4173`と`:5173`）。

## 接続範囲

- MetaMask `eth_requestAccounts` / `eth_signTypedData_v4`によるEOA認証。
- ページメモリだけに保持する短命Ed25519 identity、SessionHandle、HPKE秘密鍵。
- 入金seed、配分、Agent生成・承認、market/limit注文、取消、回収、署名付き出金、資金・注文・約定履歴。
- 個人参照と取消はCandidを一元codecで封入し、request ID再利用や復号失敗を自動再送しません。
- marketは即時約定、limitはrestingとなる決定的なLOCAL MOCK。admin APIはloopback bindだけです。

## 検証

```sh
pnpm lint
pnpm format:check
pnpm typecheck
pnpm test
pnpm build
pnpm test:e2e            # shell/CSP。実接続ケースはskip
bash ../scripts/local-e2e.sh # 実Canister一巡
node --test ../tools/mock-hl/server.test.mjs
```

通常のPlaywrightはbuild後のWorkers preview（127.0.0.1:4173）を使います。実接続E2Eだけがローカルreplicaとmockを必要とします。

## 安全境界

- Workersは`APP_STAGE=local`かつloopback requestだけを許可し、CSP `connect-src`は設定済みIC hostとmock hostだけです。
- `/fallback`は通常の取引画面や市況接続に依存せず、認証・注文取消・本人EOA宛出金だけを提供します。Canister停止を回避するものではありません。
- SSR/server functionsへ本人データを渡さず、localStorage・sessionStorage・Cookieへ鍵やセッションを保存しません。
- unknownは画面へそのまま表示し、自動再送しません。古いsnapshot、未承認Agent、未観測口座では新規注文を停止します。
- snapshotの鮮度は受信後の経過時間を加算して判定し、10秒超過または必須データ取得失敗で新規注文を停止します。取消・reduce-only操作はこの停止条件から分離しています。
- 注文の受付応答を失った場合は、HPKE経由の`get_order_by_request`で本人の受付結果を照合します。未観測は失敗確定ではなく、新規注文を再開する条件にはなりません。HTTPクライアントの自動再試行も無効です。
- ログアウト・失効時は即座にセッション世代を無効化し、口座・履歴の追加ページ・未解決要求を破棄します。古い通信の成功・失敗は新セッションへ反映しません。未解決要求は再読込後に復元できないため、再読込を解決手段にしないでください。
- mock seedは画面と応答の両方で`LOCAL MOCK`と表示します。一般ユーザー機能や本番APIではありません。
- Cloudflare公開、SNS操作、controller変更、本番資金受付は行いません。
