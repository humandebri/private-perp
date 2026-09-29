# HL testnet 受入状態

更新: 2026-09-29。**未合格**。この文書は現時点の公開環境と、受入までに必要な確認だけを示します。

| 対象 | 状態 |
| --- | --- |
| IC Canister | `xis3j-paaaa-aaaai-axumq-cai` は cycles 補充待ちで停止中 |
| [Cloudflare UI](https://private-perp-ui-testnet.hude.workers.dev) | 公開中。停止中の Canister を使う操作は現在できない |
| ローカルの最新 Wasm | HTTP outcall v2 の非複製通信、停止できるワーカー、手動再開を実装・検証済み。公開 Canister には未反映 |
| 実 HL testnet | 送金・注文 POST と資金往復は未検証 |

[ローカルのデプロイ前検証](predeploy-validation-2026-09-29.md)は公開環境の受入とは別の結果です。PocketIC の mock outcall や Cloudflare の dry-run だけで公開環境を合格にしません。

## 公開環境で必要な確認

1. Canister の cycles を補充し、アップグレードに必要な余裕を確認する。最新 Wasm を配置して起動し、module hash、管理者、tECDSA key ID `test_key_1`、HL testnet 接続先を確認する。管理者 Principal は `r75h6-lqd7b-5jack-at55d-vvti2-lg5qy-ly73a-5ezve-odnkc-kagu3-nae`。
2. 公開 UI で署名ログイン、利用資格登録、入金先表示、市況と口座状態の更新を再確認する。BTC/ETH の市場観測が新規注文を許可することを確認する。市況が不明・古い場合は停止を維持する。
3. 独立した試験用 EOA を2つ以上用意し、各口座の testnet USDC と出金に必要な残高を確認する。試験用鍵やシードを Git に保存しない。
4. 各口座で少額の入金→保管→指定額の配分→注文→取消または reduce-only 決済→回収→出金を実行する。要求 ID、外部 POST の応答、両口座の操作前後残高、資金履歴と注文履歴を秘密を除いて記録する。
5. 応答喪失時に同一の資金移動や注文を自動再送しないことを確認する。履歴が欠ける、または候補が曖昧な場合は `unknown`、予約、フェンスを保持し、未実行と決めつけない。

実 HL の履歴を欠落なく取得できることが確認されるまで `recovery_history_verified` を有効化しない。ローカル IC で読み取り専用の testnet 口座準備を再現する場合は、リポジトリルートの `scripts/prepare-local-testnet.sh` を参照してください。

## 秘匿性の判定

資金往復の機能合格と匿名性は別に判定します。現行 B0 の[合成相関評価](privacy-local-eval.md)では top-1 対応付け成功率が 100% で、[評価基準](../phase-0/privacy-evaluation.md)に未達です。配分・退出方式と実利用者コホートの評価を終えるまで、匿名取引や金額・時刻の秘匿を主張しません。
