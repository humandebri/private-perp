# 単一 Canister 化

HL testnet 用バックエンドの配置単位は `private_perp` 一つ。UI は Cloudflare Workers から単一 ID に接続する。旧五 Canister からの移行・後方互換は対象外。

## 実装と権限

`crates/private-perp` は資金、注文、policy、journal の四モジュールを一つの Wasm にリンクする。guard はリンクしない。各 SQLite は stable memory の別スロットに保持し、非同期処理も poll ごとに保存領域を選ぶ。衝突する公開メソッドには `vault_` / `core_` / `policy_` / `journal_` を付ける。

内部 principal はインストール時に自分自身へ固定し、変更 API を公開しない。認証・policy・journal の自己呼び出しはメッセージ境界を維持する。journal の `vault` と `core` は論理ストリームを分け、役割付き入口は自己呼び出しに制限する。

`init(administrator : principal)` で指定した管理者を永続化する。policy、REST 予算、cycles 設定、市場閾値、利用資格設定、journal の照合再開にはこの管理者を要求する。匿名・management canister・自分自身は指定できない。controller 全員へアプリ管理権限を自動付与しない。鍵・接続環境・移行設定等の既存 controller 操作と、IC のアップグレード権限は残る。管理者の変更 API は設けていない。

アップグレード後は vault/core の送信を停止し、指定管理者が各 journal を照合して再開する。一つのアップグレード権限が資金鍵・注文鍵・業務記録に及ぶため、独立 Canister の journal による巻き戻し検出や guard によるアップグレード猶予の保証はない。

## ビルド・検証

`candid-extractor`、`didc`、`ic-wasm` を PATH に用意する。

```sh
bash scripts/extract-candid.sh
bash scripts/generate-frontend-bindings.sh
icp build private_perp
bash scripts/test-single-canister.sh
```

Candid は元モジュールから生成し、本番 Wasm の query/update エクスポートと照合する。統合 API は 115 メソッド。`candid/private_perp.did` の init 引数と UI バインディングも同期済み。

PocketIC は管理者・非管理者の認可、署名ログイン、HPKE、口座作成、利用資格登録、残高不足、配分の冪等性と過剰配分拒否、journal の自己呼び出し、アップグレード後の永続化と照合再開を検証する。試験専用 Wasm ではさらに配分3件の署名送信、模擬着金、Agent承認、注文送信を統合経路で確認する。HL応答はmockであり、実testnet受理の証拠ではない。試験専用 Wasm は別 target に作り、本番には mock 入金入口を含めない。

## ローカルと公開

ローカル新規配置では `icp deploy private_perp --args '(principal "ADMIN_PRINCIPAL")'` とし、bootstrap を実行する identity を管理者に指定する。`scripts/local-e2e.sh` はこの指定と単一 ID の設定を行う。既存ローカルネットワークを共用している場合は実行しない。公開環境のブラウザーで署名ログイン、資格登録、入金先表示まで実接続を確認した。

UI は `VITE_PRIVATE_PERP_CANISTER_ID` 一つを使う。公開試験用の例は `frontend/.env.testnet.example`。設定を `.env.testnet.local` に保存し、`pnpm --dir frontend build --mode testnet` で `wrangler.testnet.jsonc` を選ぶ。testnet 画面は HL testnet 上での送金先・送金元を案内し、mock 入金を実行しない。

2026-09-28、ユーザー指定の既存 `xis3j-paaaa-aaaai-axumq-cai` を再インストールし、統合 Wasm を公開 ICP に配置した。管理者は `r75h6-lqd7b-5jack-at55d-vvti2-lg5qy-ly73a-5ezve-odnkc-kagu3-nae`。以前の Wiki データは再インストールで消去された。スナップショットは cycles 不足で作成できなかった。UI は [Cloudflare Workers](https://private-perp-ui-testnet.hude.workers.dev) に公開した。設定と未完了の受入条件は[受入記録](testnet-acceptance.md)を参照。
