# ADR-0002：口座とmaster/Agent権限の分離

- 日付：2026-09-18
- 設計状態：accepted
- 実証状態：未実装・未実証。UIデモは資金・暗号化・ガバナンスの証明ではない

## 背景

HL標準の証拠金・清算を使い、ユーザー間の損失共有を避ける必要がある。

## 比較した案

ユーザーがmasterを持つ方式、共通masterのsub-account、ユーザー別独立masterをCanisterが管理する方式を比較した。

## 決定

取引しない共通保管口座と、ユーザー別独立HL取引口座を使う。funds_vaultがmaster署名、trading_coreが口座別・世代別Agent署名を担当する。EOAは認証と出金意図を署名し、master秘密鍵は受け取らない。初期払出し先は認証EOAのHL口座に限定する。

## 欠点・残存リスク

ユーザー単独の直接出金・Agent解除はできない。Canister分離だけでは同じ変更権限者による侵害を防げない。共通保管資産と取引口座equityの二重計上を防ぐ必要がある。

## 再検討条件

HLの口座・Agent・送金仕様変更、鍵の移行、単独回収要件が発生した場合。

## 検証の正

実行結果・未実装範囲はfrontend/README.mdとdocs/implementation-status.mdで追跡する。既存のPlan.md 16章の本番ゲートを省略しない。

