# ADR-0005：TanStack Startと限定的なUI再利用

- 日付：2026-09-18
- 設計状態：accepted
- 実証状態：UI基盤・デモのローカル検証対象。本番未検証

## 背景

Next.jsを避けつつ、HL利用者が慣れた高品質の取引UIを作りたい。

## 比較した案

Start＋React、Preact＋Vite、Start＋Preact互換層、既存アプリ全体のforkを比較した。

## 決定

Start＋React＋TypeScript＋Vite、TanStack Router/Query/Table、Tailwind、Lightweight Chartsを採用する。pnpmで依存を完全固定し、Oxlint＋型対応、Oxfmt、tsc、Vitest、Playwrightを使う。React Compilerは初期採用しない。HypeTerminalは部品の参考候補のみとし、初期UIは独自作成する。

## 欠点・残存リスク

Preactより軽量とは限らない。HL並みの全機能・執行速度は保証しない。Chartの描画・操作機能は段階追加。Tableは検証したv8系APIに固定し、v9への追従は別の変更として試験する。

## 再検討条件

UI再利用による短縮効果が確認できた場合はライセンス・依存・通信処理を監査して部品単位で採用する。Next.jsやPreactへの自動変更はしない。

## 検証の正

実行結果・未実装範囲はfrontend/README.mdとdocs/implementation-status.mdで追跡する。既存のPlan.md 16章の本番ゲートを省略しない。

