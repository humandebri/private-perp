# private-perp UI

TanStack Start＋ReactをCloudflare Workersのローカルランタイムで配信する、合成データ専用プロトタイプです。資金・署名・注文バックエンドはまだありません。

## 起動

```sh
cd frontend
corepack pnpm install --frozen-lockfile
pnpm cf:typegen
pnpm dev
```

Node 24系、pnpm 12.4.2を使用。既存の実装・Canister ID・Candidが存在しないため、MetaMask、HPKE、実USDC、実HL注文、ICP認証は接続していません。勝手に互換APIを作って接続済みと見せない方針です。

## 検証

```sh
pnpm lint
pnpm format:check
pnpm typecheck
pnpm test
pnpm build
pnpm test:e2e
```

Playwrightの対応Chromiumが必要です。未導入環境では、管理者の方針に従って `pnpm exec playwright install chromium` を実行してください。テストはbuild後のWorkers previewを使います。ローカルpreviewは4173、開発は5173です。

既存のテスト専用Chromiumを使う場合は、`PLAYWRIGHT_CHROMIUM_EXECUTABLE`にその実行ファイルの絶対パスを指定できます。今回のローカルE2Eは既存Chromium revision 1228で確認し、普段使いのChromeは操作していません。CIでは固定したPlaywrightに対応するChromiumを取得します。CI workflowは追加済みですが、リモートCIは未実行です。

画面の「VEIL」は作業用の仮称です。製品名・商標の確定ではありません。

## できること

- 公開説明、取引、資金、履歴の4画面。SSRは公開情報のみ。
- 合成チャート・更新する合成板・注文入力・注文状態一覧。
- 部分約定、拒否、unknown、取消競合、古い口座情報の再現。
- 合成残高の追加・配分・回収・払出しと確認画面。
- request IDの同一要求再送抑止と、異なる本文の拒否。
- デモ終了時の状態・タイマー・Queryキャッシュ破棄。

## 未実装と安全境界

- 全価格・残高は合成。公開HL市況への接続もまだありません。
- 建玉・PnL・SL/TP・決済は未接続表示。デモ約定で建玉や証拠金を捏造しません。
- 資金デモは整数残高モデルであり、本番の複式台帳・永続outbox・復旧を代替しません。
- 再読込でセッションは消えます。本番unknownを再読込で破棄してよいという意味ではありません。
- SSR/server functionsへ本人データを渡さず、ブラウザ保存領域も使用しません。
- 外部データ通信・財布接続はありません。WorkersはGET/HEADのみ。APP_STAGEがdemo以外なら503。
- 地域制限は国コードの入口例のみ。VPN検出・sanctions・eligibility発行は未実装です。
- CSPはframe/base/object/formの制限のみ。Startのscript用nonceと本番connect-srcの設計は接続API確定後の必須作業です。
- Cloudflare配信権限は資金処理とは別の信頼点です。独立guardの7日猶予はUI配信には適用されません。

## 再利用・ライセンス

HypeTerminal、Defuseのコードや画像はコピーしていません。画面は独自実装です。Lightweight Chartsの表示にTradingView attributionを設けています。依存物のNOTICE・配布条件は本番リリース前に再確認してください。

## 本番接続に必要な入力

ICP側の実装、Candid、networkとCanister ID、認証・失効契約、認証済みHPKE鍵取得・暗号化仕様、署名・注文・資金照合の試験fixtureが必要です。これらが揃うまでは本番モード・実資金操作を有効化しません。

## 公開について

デプロイコマンドは自動実行しません。Cloudflare公開、SNS操作、controller変更、本番資金受付は行っていません。
