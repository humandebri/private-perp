# Phase 3 testnet受入記録

更新: 2026-09-28。状態: **未合格**。[2026-09-26のローカルCanister + HL testnet](local-canister-hl-testnet.md)では旧5 Canister構成で口座準備・認証・実市場読取りまで成功した。現行の単一 `private_perp` Wasmでは実HLの送金・注文POST、実資金往復、公開ICPでの受入は未実施。PocketICのmock outcallによる結合試験と区別する。

## GATE 0

| 条件 | 現在の確認結果 |
|---|---|
| testnet用IC identity | 未確認。作業環境の`.icp-home`には`private-perp-local`とanonymousのみ |
| 専用の単一 `private_perp` ID、設置時の管理者Principal、cycles | 未確認。`icp.yaml`はlocal networkのみ。旧5 IDは使用しない |
| tECDSA key IDと署名可否 | 未確認 |
| 独立したHL testnet口座2つ以上 | 未確認 |
| 各口座のtest USDCと預入・回収に必要な残高 | 未確認 |

秘密鍵、シード、API walletの署名秘密をこの記録やgit管理下に置かない。GATE 0の証明にはidentity名・canister ID・key ID・口座公開アドレス・残高の読み取り結果を使う。

## 受入証跡

GATE 0成立後、少額の預入→配分→注文→取消またはreduce-only決済→回収→出金を口座ごとに実施し、各操作の要求ID、`usdSend`応答、送受金両口座の操作前後残高、`userNonFundingLedgerUpdates`の生JSON型と対象イベントを秘密を除いて記録する。POST応答喪失を注入した回収は再送せず、履歴から受理を確認する。履歴が完全に取得できない、または候補が曖昧な場合は`unknown`、予約、フェンスを保持する。

ローカルICで実HL testnetの口座準備だけを再実行する場合は、空の専用ローカルネットワークへ単一Wasmを設置し、`TESTNET_APP_ID`を明示して`scripts/prepare-local-testnet.sh`を実行する。スクリプトはCanister IDと現identityのアプリ管理者一致を検査し、既存Canisterのデプロイや初期化は行わない。実資金移動を始める前に専用環境と残高を別途確認する。

実HLで期間取得の欠落がないことを確認できるまでは`recovery_history_verified`を有効化せず、未実行判定による自動解除を合格にしない。ローカル負荷と相関評価はそれぞれ[混合負荷](mixed-load-local.md)と[相関評価](privacy-local-eval.md)で別判定する。

秘匿性は資金往復の機能合格と別に判定する。現行B0は合成評価でtop-1対応付け成功率100%であり、[相関評価の基準](../phase-0/privacy-evaluation.md)（20人以上の実観測を加えた未見評価でtop-1 20%以下、対照比80%以上削減、直接一意に辿れる割合5%以下）を満たしていない。B1の配分・退出方式と実利用者コホートの評価が終わるまで匿名取引や金額・時刻の秘匿を主張しない。単独・低利用では合格値を満たしても保証しない。
