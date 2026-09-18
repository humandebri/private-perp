# ADR-0006：Workers配信とICP処理の分離

- 日付：2026-09-18
- 設計状態：accepted
- 実証状態：UI基盤・デモのローカル検証対象。本番未検証

## 背景

配信・公開SSRを簡素化しつつ、機密性と資金管理の責務をICPに保つ。

## 比較した案

ICP asset canister、Workers＋Static Assets、独自Node.jsサーバーを比較した。

## 決定

Workers＋Static Assetsを主配信にする。公開ページをSSR、取引・資金・履歴をクライアント描画にする。資金DB・注文API・署名はWorkersへ移さない。Express/Hono/D1/KV/R2/DOや独自WS中継を初期追加しない。入口地域制限はCanister側eligibilityの代わりではない。

## 欠点・残存リスク

Cloudflareと配信権限者が実行JSを変えられる。ICPの7日猶予は配信変更を拘束しない。初期はデモ専用でGET/HEADのみ、実モードは503にする。国判定はCFメタデータのみ使用し、デモのローカル実行ではメタデータ不在を許す。本番の国・VPN・screening・token発行は未実装。

## 再検討条件

ICP主配信、追加サーバー機能、本番eligibility発行を導入する場合。本番前に権限分離、レビュー、配信物の検証と依存監査を完了する。

## 検証の正

実行結果・未実装範囲はfrontend/README.mdとdocs/implementation-status.mdで追跡する。既存のPlan.md 16章の本番ゲートを省略しない。

