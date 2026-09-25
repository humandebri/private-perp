# Phase 3 testnet受入記録

更新: 2026-09-25。状態: **実装完了まで保留・未合格**。ユーザーの指示によりtestnet受入はローカル実装の後に行う。実HLへの送信は行っていない。

## GATE 0

| 条件 | 現在の確認結果 |
|---|---|
| testnet用IC identity | 未確認。作業環境の`.icp-home`には`private-perp-local`とanonymousのみ |
| vault/core/policy/guard/journalのtestnet canister IDとcycles | 未確認。`icp.yaml`はlocal networkのみ |
| tECDSA key IDと署名可否 | 未確認 |
| 独立したHL testnet口座2つ以上 | 未確認 |
| 各口座のtest USDCと預入・回収に必要な残高 | 未確認 |

秘密鍵、シード、API walletの署名秘密をこの記録やgit管理下に置かない。GATE 0の証明にはidentity名・canister ID・key ID・口座公開アドレス・残高の読み取り結果を使う。

## 受入証跡

GATE 0成立後、少額の預入→配分→注文→取消またはreduce-only決済→回収→出金を口座ごとに実施し、各操作の要求ID、`usdSend`応答、送受金両口座の操作前後残高、`userNonFundingLedgerUpdates`の生JSON型と対象イベントを秘密を除いて記録する。POST応答喪失を注入した回収は再送せず、履歴から受理を確認する。履歴が完全に取得できない、または候補が曖昧な場合は`unknown`、予約、フェンスを保持する。

実HLで期間取得の欠落がないことを確認できるまでは`recovery_history_verified`を有効化せず、未実行判定による自動解除を合格にしない。ローカル負荷と相関評価はそれぞれ[混合負荷](mixed-load-local.md)と[相関評価](privacy-local-eval.md)で別判定する。
