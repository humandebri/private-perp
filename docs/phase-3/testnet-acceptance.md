# Phase 3 testnet受入記録

更新: 2026-09-26。状態: **未合格**。ユーザーの指示により、まず[ローカルCanister + HL testnet](local-canister-hl-testnet.md)の接続試験を開始した。口座準備・認証・実市場読取りは成功し、テストUSDCの入金待ち。実HLへの注文・資金移動POSTは未実施。公開ICPでの受入は引き続き別判定とする。

## 実口座の残高確認（2026-09-30）

`https://api.hyperliquid-testnet.xyz/info`の`clearinghouseState`で、保存済みの公開アドレスを再確認した。本人用テスト口座`0x08ef566005b8f2b5ed94273add6cbbb414fe3bab`と共通保管用テスト口座`0xf9b2b86555bde4bd83ce5b5590ae56d0fd9d1a0f`は、どちらも`accountValue=0.0`、`withdrawable=0.0`、建玉0件だった。実取引・資金移動のPOSTは実行していない。実HLでの通し検証は、テスト資金の用意と現在のローカルcanister状態・口座の対応確認を済ませてから行う。

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
