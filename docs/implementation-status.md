# 実装状況：UI・サーバー基盤とADR

更新：2026-09-18。対象はローカル開発用の合成デモ。全Phase完了ではない。

## 完了したもの

- ADR 0001〜0006。設計採用と実証状態を分離した。
- Plan v0.9、Implementation v0.5、ロードマップv1.1への整合。
- 新規TanStack Start＋React＋TypeScript/Vite、Workers＋Static Assetsのローカル配信。
- pnpm固定依存・lockfile、Workers生成型、Oxlint型対応、Oxfmt、tsc、Vitest、Playwright。
- `/`、`/trade`、`/funds`、`/history`。公開ページSSR、口座画面はクライアント描画。
- Lightweight Charts、合成板、注文フォーム、TanStack Table注文一覧、資金確認・履歴。
- 正常受理・部分約定・拒否・unknown・取消競合・古い状態のシミュレーション。
- 合成資金の整数計算、要求IDの冪等性、ログアウト時のメモリ破棄。
- WorkersのGET/HEAD限定、実モード503、CF国コードによる入口制限例、セキュリティヘッダー。
- GitHub Actionsの検証workflow追加。リモートでの実行は未実施。

## 検証結果

- `pnpm build`：成功。Cloudflare Workers向けのビルド。
- `pnpm typecheck`：成功。
- `pnpm lint`：成功。型対応あり。Table v8についてReact Compiler非採用を理由とする1行限定の除外がある。
- `pnpm format:check`：成功。
- `pnpm test`：14件成功。注文状態・資金整数/冪等性・入口制限・ヘッダー。
- `pnpm test:e2e`：4件成功。build後のWorkers preview、既存テスト用Chromium revision 1228を使用。
- Playwright CLIによる画面表示・操作・コンソール確認。スクリーンショットは引渡し成果物に保存。

ブラウザ試験でJS/CSS配信の404とチャートautoSizeのレイアウト変動を検出・修正した。ビルド成功だけをUI完成とは扱っていない。Safari・Firefox、実MetaMask、実ICP、性能負荷、実資金の試験は未実施。

## 未実装・次工程を止めている条件

対象ディレクトリにCanisterコード、Cargo workspace、Candid、testnet Canister IDがない。そのため本番APIを推測して作らず、ICP接続段階を保留している。

必要な次の成果物：

1. Rust/PocketICでの資金・署名・認証の実装と検証。
2. Candid、network・Canister ID、本人認証・失効の実契約。
3. 認証済みHPKE公開鍵の取得・鍵更新・要求と応答の暗号化仕様。
4. 注文・資金移動の照合fixtureと、unknownの回復契約。
5. HL公開市況の接続、口座状態・建玉・PnL・SL/TP・決済の接続。

画面に建玉プレビューはあるが、SL/TP・決済は無効表示。デモで約定したことを根拠に本物の建玉を作らない。単体テストの整数残高モデルは本番の複式台帳や永続outboxを代替しない。

## 本番前の残件

Canisterの資金安全性・機密性・相関耐性、SNS/guard、controller移管、法務、eligibility発行、配信権限分離、nonce対応を含むCSP、依存ライセンス・NOTICEの再確認、独立監査が必要。Cloudflare配信権限が持つJS変更リスクは残る。

Cloudflare公開、SNSローンチ、controller変更、ウォレット接続、実資金操作は行っていない。
