# Phase 3 testnet受入記録

## 2026-09-29 更新

[市場判定に対する合意](market-consensus.md)の修正を実装し、本番・試験用 Wasm の PocketIC、115公開メソッドの Candid一致、Clippy を確認した。公開アップグレードは cycles 不足で拒否され、未反映。IC は約285.3B cyclesの追加を要求した。復旧用スナップショットも約1.34B不足で作成できなかった。

指定 Canister は消費抑制のため **停止中**。拒否後の module hash は従前の `3cb739a6273717b0399b83eb4ec7251dafd0689394b898aecda346b96b7383fc` で、旧Wasmとデータを維持している。実行用残高は約85.6B cycles。Cloudflare UIは公開中だが、Canister操作は補充・アップグレード・起動後に再確認する必要がある。新Wasmは `060b61d4bfaaec525c7bb5a6e4dae4afb9a157ee9606ad2515535e76817ceedb`。公開subnetでの修正確認と実送金・注文は未実施。

以下は2026-09-28の公開時点の記録。

更新: 2026-09-28。状態: **未合格**。[2026-09-26のローカルCanister + HL testnet](local-canister-hl-testnet.md)では旧5 Canister構成で口座準備・認証・実市場読取りまで成功した。現行の単一 `private_perp` Wasmを公開ICPに配置し、Cloudflare UIから署名ログイン、利用資格登録、入金先表示まで確認した。実HLの送金・注文POST、資金往復は未実施。PocketICのmock outcallによる結合試験と区別する。

## GATE 0

| 条件 | 現在の確認結果 |
|---|---|
| testnet用IC identity | `llm-wiki-mainnet` を既存 Canister の controller と管理者に使用 |
| 単一 `private_perp` ID、管理者Principal、cycles | `xis3j-paaaa-aaaai-axumq-cai`、`r75h6-lqd7b-5jack-at55d-vvti2-lg5qy-ly73a-5ezve-odnkc-kagu3-nae`。設定後約961B cycles |
| tECDSA key IDと署名可否 | `test_key_1` を設定。実署名・HL送金は未検証 |
| 独立したHL testnet口座2つ以上 | 試験用EOA Bで署名ログインと資格登録を確認。ユーザーのEOA Aでの登録、2口座同時利用は未検証 |
| 各口座のtest USDCと預入・回収に必要な残高 | 未確認。実送金は未実施 |

公開 UI: <https://private-perp-ui-testnet.hude.workers.dev>。HL API/市況WebSocketは testnet、IC hostは `https://icp-api.io`。利用資格 issuer 公開アドレスは `0x79a503BfcDA54a66490E257d81C64980aD0fb60E`。署名秘密は git 管理外に保存する。testnet の試験用EOA Bで、公開UIの署名ログイン、資格登録、共通保管口座 `0xc59960cb75dbd9d92f9e5b3fc1833acb7315a378` の表示を確認した。Bを送金元にした試験であり、ユーザーのEOA Aの送金元確認や着金計上を示すものではない。

BTC/ETH の市場観測は現在 `market_observation_stale` で新規注文を停止する。変動する HL `/info` 応答を複数 IC ノードが同一内容として取得できず、明示的な更新では `No consensus could be reached. Replicas had different responses` が返った。市場取得の修正と再検証が完了するまで注文の受入を合格にしない。`recovery_history_verified=false`、emergency stop は解除済み。

秘密鍵、シード、API walletの署名秘密をこの記録やgit管理下に置かない。GATE 0の証明にはidentity名・canister ID・key ID・口座公開アドレス・残高の読み取り結果を使う。

## 受入証跡

GATE 0成立後、少額の預入→配分→注文→取消またはreduce-only決済→回収→出金を口座ごとに実施し、各操作の要求ID、`usdSend`応答、送受金両口座の操作前後残高、`userNonFundingLedgerUpdates`の生JSON型と対象イベントを秘密を除いて記録する。POST応答喪失を注入した回収は再送せず、履歴から受理を確認する。履歴が完全に取得できない、または候補が曖昧な場合は`unknown`、予約、フェンスを保持する。

ローカルICで実HL testnetの口座準備だけを再実行する場合は、空の専用ローカルネットワークへ単一Wasmを設置し、`TESTNET_APP_ID`を明示して`scripts/prepare-local-testnet.sh`を実行する。スクリプトはCanister IDと現identityのアプリ管理者一致を検査し、既存Canisterのデプロイや初期化は行わない。実資金移動を始める前に専用環境と残高を別途確認する。

実HLで期間取得の欠落がないことを確認できるまでは`recovery_history_verified`を有効化せず、未実行判定による自動解除を合格にしない。ローカル負荷と相関評価はそれぞれ[混合負荷](mixed-load-local.md)と[相関評価](privacy-local-eval.md)で別判定する。

秘匿性は資金往復の機能合格と別に判定する。現行B0は合成評価でtop-1対応付け成功率100%であり、[相関評価の基準](../phase-0/privacy-evaluation.md)（20人以上の実観測を加えた未見評価でtop-1 20%以下、対照比80%以上削減、直接一意に辿れる割合5%以下）を満たしていない。B1の配分・退出方式と実利用者コホートの評価が終わるまで匿名取引や金額・時刻の秘匿を主張しない。単独・低利用では合格値を満たしても保証しない。
