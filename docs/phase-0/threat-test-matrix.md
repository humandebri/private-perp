# 脅威と試験の対応表

- 根拠：`Implementation.md` 9.2〜9.4、14.4、`Plan.md` 3.2、16章、ロードマップ4章・6章
- 状態：設計契約。**一部実行済み**。実行済みの試験は実施日と証跡を付す（2026-09-19にT-101〜T-105を実行）。未実行のものは未実行のまま

## 1. 読み方

- 「層」は試験の実施手段: `unit`（純粋関数・ホスト）、`PocketIC`（Canister統合）、`testnet`（HL実接続）、`Playwright`（UI）、`手動`（環境・運用）、`レビュー`（コード・設定の静的確認）。
- 「証跡」は合格時に残す記録。資金・署名・認証に関わる証跡は保護し、平文のintent・署名・対応表を残さない。
- 実装Phaseは、その試験をどのPhaseで実行可能になるかを示す。Phase 0では契約のみを固定する。

## 2. 必須の検証（ロードマップ5章・6章）との対応

| ロードマップ項目 | 対応する試験 |
|---|---|
| 入金を一度だけ計上し、未確定の送金を確定残高へ含めない | T-201、T-202、T-204 |
| 出金の本人署名、宛先、金額、期限、nonceを検証する | T-102、T-104、T-203 |
| 送金成功後の応答喪失でも自動再送で二重払出ししない | T-205、T-206 |
| trading_coreからmaster署名や任意出金を要求できない | T-301、T-302 |
| ブラウザからHLへユーザー別取引口座の照会が出ない | T-601、T-602 |
| 署名・照合の遅延、outcall/署名/保存コストを記録する | T-801（計測。合否は`Implementation.md` 2.4） |
| Confidential Subnetの利用可否と未検証の信頼仮定を記録する | T-802 |

## 3. 認証・セッション

| ID | 脅威・失敗条件 | 対策（契約） | 層 | 期待結果 | 証跡 | Phase |
|---|---|---|---|---|---|---|
| T-101 | 偽のEOAでchallengeを解く | EIP-712 `typed_data` にorigin/network/canister/用途/nonce/期限を束縛 | unit, PocketIC | 拒否（`BadRequest.InvalidSignature`） | 試験ログ | 1 |
| T-102 | 別Principalでセッションを開く | challengeの`principal`と署名EOAを束縛 | PocketIC | 拒否 | 試験ログ | 1 |
| T-103 | 期限切れchallengeの再利用 | 5分期限、一回性nonce | unit, PocketIC | 拒否（`ChallengeExpired`） | 試験ログ | 1 |
| T-104 | challenge再使用（二重`open_session`） | nonce一回性をDBの一意制約で保証 | PocketIC | 2回目を拒否（`ChallengeReused`） | 試験ログ | 1 |
| T-105 | 別origin・別networkからの要求 | `aad`とchallengeの束縛 | unit, PocketIC | 拒否（`OriginMismatch`／`NetworkMismatch`） | 試験ログ | 1 |
| T-106 | `purpose = withdrawal` のchallengeでセッション確立 | 用途分離 | PocketIC | 拒否 | 試験ログ | 1 |
| T-107 | ログアウト後も旧セッションが有効 | `revoke_session`で失効世代を更新 | PocketIC | 失効（`SessionRevoked`）。資金要求・新規リスク受付を拒否 | 試験ログ | 2 |
| T-108 | 失効伝達前に`trading_core`を呼ぶ | 未確認セッションを受理しない | PocketIC | 拒否（`SessionIssuedByUnregisteredVault`等） | 試験ログ | 2 |
| T-109 | セッション鍵・本人キャッシュのブラウザ永続化 | 永続化しない設計 | Playwright, レビュー | 保存領域に残らない | 画面記録 | 2 |

実行済み（2026-09-19、`crates/pocket-ic-tests/tests/vault_auth.rs`）: T-101（別鍵の署名を拒否）、T-103（期限切れを拒否）、T-104（challenge再使用を拒否）、T-105（別origin/networkの署名を拒否）。T-102は未実行。

実行済み（2026-09-19、`crates/pocket-ic-tests/tests/vault_outbox.rs`）: T-205（送金成功後の応答喪失で自動再送しない）。T-206は「未解決actionを再送しない」部分のみ確認（時間経過での解放は未実装）。

## 4. 資金・台帳・出金

| ID | 脅威・失敗条件 | 対策 | 層 | 期待結果 | 証跡 | Phase |
|---|---|---|---|---|---|---|
| T-201 | 入金イベントの重複計上 | 外部安定IDの一意制約 | PocketIC | 2件目を拒否。残高不変 | 試験ログ | 1 |
| T-202 | 仕訳不均衡（借方≠貸方） | journal単位の検査 | unit | 拒否して書き込まない | 試験ログ | 1 |
| T-203 | 宛先差替え・別networkの出金intent | intent署名の束縛（金額・宛先・network・nonce・期限） | unit, PocketIC | 拒否（`DestinationNotAllowed`／`NetworkMismatch`） | 試験ログ | 1 |
| T-204 | 未確定の入金・未実現PnLを出金可能額へ算入 | 勘定分離と与信規則 | unit, PocketIC | `withdrawable`に含まれない | 試験ログ | 1 |
| T-205 | 送金成功後に応答を喪失し、自動再送で二重払出し | `dispatching`永続化後にのみ送信、`unknown`は自動再送しない | PocketIC | 二重送金なし。`unknown`を保持 | 試験ログ | 1 |
| T-206 | `unknown`を期限経過で解放し再送 | 時間経過で解放しない | PocketIC | 予約保持。再送なし | 試験ログ | 1 |
| T-207 | 並行出金で残高を超えて払出す | 予約の原子的確保、口座単位ロック | PocketIC | 合計が残高を超えない | 試験ログ | 1 |
| T-208 | 回収中の発注で移動中の証拠金を利用可能と数える | 資金移動ロック・世代で調整 | PocketIC | 新規リスク増加を拒否 | 試験ログ | 3 |
| T-209 | 他人の残高へ損失を付け替える | ユーザー別勘定 | PocketIC | 他ユーザーの残高不変 | 試験ログ | 3 |
| T-210 | master nonce・outboxの巻戻し後の自動再署名 | 復元時は送信停止から照合 | PocketIC | 自動再署名しない | 試験ログ | 3 |
| T-211 | 入金の成功表示だけで計上する | 安定イベントIDの検証必須 | PocketIC, testnet | 計上しない | 試験ログ | 1 |
| T-212 | 二重ingress（同一`client_request_id`の同時送信） | `UNIQUE(user_id, client_request_id)` | PocketIC | 1件のみ受付。敗者は`DuplicateIgnored` | 試験ログ | 2 |

## 5. 権限・署名分離

| ID | 脅威・失敗条件 | 対策 | 層 | 期待結果 | 証跡 | Phase |
|---|---|---|---|---|---|---|
| T-301 | `trading_core`からmaster署名を要求 | `funds_vault`に任意digest署名APIを設けない | レビュー, PocketIC | 該当APIが存在しない | レビュー記録 | 1 |
| T-302 | `trading_core`から任意出金を要求 | 呼出元・目的・金額・宛先・本人認可・残高の検証 | PocketIC | 拒否 | 試験ログ | 1 |
| T-303 | master鍵とAgent鍵の取り違え | 鍵の保持主体を分離し、用途を限定 | レビュー, PocketIC | 取り違え経路なし | レビュー記録 | 1 |
| T-304 | 失効世代のcallbackで状態を書き換える | `worker_epoch`と世代のCAS | PocketIC | 更新0件。結果を破棄 | 試験ログ | 2 |
| T-305 | 任意Agentアドレスの承認依頼 | 口座・世代・導出公開鍵の照合 | PocketIC | 拒否 | 試験ログ | 2 |
| T-306 | 運営権限による任意送金・出金先変更 | 該当APIを設けない | レビュー | APIが存在しない | レビュー記録 | 1 |
| T-307 | ユーザー入力の`caller`値を信用 | ICメッセージのcallerのみで認可 | unit, PocketIC | なりすまし不可 | 試験ログ | 1 |

## 6. 冪等性・注文状態

| ID | 脅威・失敗条件 | 対策 | 層 | 期待結果 | 証跡 | Phase |
|---|---|---|---|---|---|---|
| T-401 | 同一`request_id`・異なる本文 | 本文fingerprint比較 | unit, PocketIC | 拒否（`IdempotencyConflict`） | 試験ログ | 2 |
| T-402 | 同一`request_id`・同一本文の再送 | 受理済み結果を返す | PocketIC | 二重注文なし、同一応答 | 試験ログ | 2 |
| T-403 | `dispatching`以降の自動再注文 | 自動再送を禁止 | PocketIC | 新cloid・新nonceが発行されない | 試験ログ | 2 |
| T-404 | バッチ内の一部拒否を全部成功として扱う | `action_orders`で子注文ごとに照合 | PocketIC | 子注文ごとの状態が正しい | 試験ログ | 2 |
| T-405 | 部分約定後の取消を`cancelled`と表示 | `cancel_requested`と累積約定量の分離 | unit, PocketIC | `partially_filled`を維持 | 試験ログ | 2 |
| T-406 | `orderStatus`不在を未実行と断定 | 保持期間・可視化遅延を考慮し`unknown`維持 | PocketIC | `rejected`にしない | 試験ログ | 2 |
| T-407 | `dispatching`中のupgrade | 照合workerで再開 | PocketIC | `unknown`から照合できる | 試験ログ | 2 |
| T-408 | 署名中のkill-switch・取消 | 未送信actionのepoch無効化 | PocketIC | `aborted`。送信済みはcancel action | 試験ログ | 2 |
| T-409 | 送信ボタン連打 | 冪等性キー＋ローカルpending表示 | Playwright | 二重注文なし | 画面記録 | 2 |
| T-410 | 結果不明時に再発注を促す表示 | 表示規約 | Playwright, レビュー | 再発注導線を出さない | 画面記録 | 2 |

## 7. guard・変更権限

| ID | 脅威・失敗条件 | 対策 | 層 | 期待結果 | 証跡 | Phase |
|---|---|---|---|---|---|---|
| T-501 | 非SNS principalからの予約 | caller検証 | PocketIC | 拒否 | 試験ログ | 1 |
| T-502 | 7日未満での実行 | `executable_at`検査 | PocketIC | 拒否（`UpgradeTooEarly`） | 試験ログ | 1 |
| T-503 | 予約WASM hashの差替え | `wasm_hash`照合 | PocketIC | 拒否（`UpgradeContentMismatch`） | 試験ログ | 1 |
| T-504 | 引数hashの差替え | `arg_hash`照合 | PocketIC | 拒否 | 試験ログ | 1 |
| T-505 | controller追加・reinstall・stop・deleteによる迂回 | 該当APIを設けない。guard自身のcontrollersを空にする | PocketIC, 手動 | 迂回不可 | 試験ログ | 1, 4 |
| T-506 | 予約内容変更で猶予を実質短縮 | 変更は取消＋新規予約（新しい7日） | PocketIC | 新しい猶予が開始される | 試験ログ | 1 |
| T-507 | SNS自体のupgrade後の悪意ある呼出し | guard側の7日猶予を省略しない | PocketIC | 猶予なしの実行を拒否 | 試験ログ | 4 |
| T-508 | 緊急停止の解除・制限緩和を即時実行 | 解除は記録したSNS経路 | PocketIC, 手動 | 即時緩和不可 | 試験ログ | 4 |
| T-509 | 公開proposal・公開ログへの機密情報混入 | 公開するのは対象ID・hash・時刻・状態のみ | レビュー, 手動 | 顧客情報が出ない | レビュー記録 | 4 |

## 8. データ保護・プライバシー

| ID | 脅威・失敗条件 | 対策 | 層 | 期待結果 | 証跡 | Phase |
|---|---|---|---|---|---|---|
| T-601 | ブラウザからHLへユーザー別取引口座の照会・購読 | 公開市況のみ直結 | Playwright（network記録）, レビュー | 取引口座アドレスの送信なし | network記録 | 2 |
| T-602 | ユーザー系WSチャネルの購読 | 初期実装しない | レビュー | 該当チャネルなし | レビュー記録 | 2 |
| T-603 | 平文の注文・署名・対応表のログ出力 | ログ規約 | unit, レビュー | 平文が出ない | 検査記録 | 2 |
| T-604 | 他人のqueryで残高・注文を取得 | 本人認可をqueryでも要求 | PocketIC | 拒否 | 試験ログ | 1 |
| T-605 | HPKE鍵更新中の受付・照合 | 鍵ID・期限の検証と旧鍵の扱い | PocketIC | 新規受付は新鍵のみ。照合データを期限だけで破棄しない | 試験ログ | 1 |
| T-606 | 公開市況WSから約定を推測して確定表示 | 本人状態はCanister照合値のみ | Playwright, レビュー | 推測で確定しない | 画面記録 | 2 |
| T-607 | 署名対象・送信payloadのアクセス制御漏れ | 機密データとして制御 | レビュー | 制御外から読めない | レビュー記録 | 2 |
| T-608 | 配信権限者によるJS改変（guard猶予の対象外） | 依存固定・配信権限分離・CSP・リリースレビュー | 手動, レビュー | 未実施であることを明示 | 運用記録 | 4 |

## 9. 環境・障害・復旧

| ID | 脅威・失敗条件 | 対策 | 層 | 期待結果 | 証跡 | Phase |
|---|---|---|---|---|---|---|
| T-701 | mock tokenが本番設定で通る | `environments.md` E-1 | 手動 | 拒否 | 試験記録 | 1 |
| T-702 | 署名キュー溢れ | 有界バックオフ、失敗を注文の失敗にしない | PocketIC | 再試行に回る | 試験ログ | 1 |
| T-703 | HL停止中の新規注文 | cancel-onlyモード、新規リスク増加停止 | PocketIC, testnet | 新規停止、取消は成功 | 試験ログ | 2 |
| T-704 | cycles不足 | 段階的な受付停止（30日目標/7日通知/3日停止） | PocketIC, 手動 | 新規預入・新規リスク受付を停止 | 試験記録 | 3 |
| T-705 | 古いDBバックアップからの復元 | 送信停止→照合→解除判断 | PocketIC | 自動再開しない | 試験ログ | 3 |
| T-706 | Canister upgrade（各状態） | init/post_upgradeで`Db::init`→`migrate`、epoch fencing | PocketIC | 状態が回復し、二重実行しない | 試験ログ | 2 |
| T-707 | 安定メモリ成長失敗・`ZeroExtentLimitExceeded` | 容量インシデントとして扱う | PocketIC | panicせず回復可能エラー | 試験ログ | 3 |
| T-708 | データ鮮度超過（10秒超） | 新規リスク増加を停止し理由を表示 | unit, Playwright | 受付拒否。時刻を表示 | 試験ログ | 2 |
| T-709 | 再接続・更新順序の逆転で古い状態を最新として上書き | `revision`・`observed_at`の比較 | Playwright, unit | 上書きしない | 画面記録 | 3 |
| T-710 | 公開市況WSの切断・スナップショット | 再接続時に`isSnapshot`で置換、ping送信 | Playwright, unit | 差分として誤適用しない | 画面記録 | 2 |

## 10. 計測（合否はPhase 1のGo/No-Go）

| ID | 項目 | 出力 | Phase |
|---|---|---|---|
| T-801 | `sign_with_ecdsa` p50/p95、受付→HL受理p50/p95、キュー溢れ率、outcall所要、REST weight帰属、署名/outcall/保存コスト | 計測レポート | 1 |
| T-802 | Confidential Subnetの利用可否、outcall・upgrade・復旧の動作、未検証の信頼仮定 | 記録（信頼仮定を明示） | 1 |
| T-803 | A/B0/B1相関評価（`privacy-evaluation.md`） | 評価レポート | 1 |

## 11. 未実行であることの明示

- 本表の試験はすべて設計であり、2026-09-19時点で1件も実行していない。
- UIデモの成功、ユニットテストの成功、ビルド成功を、本表の試験合格として扱わない。
- 資金・署名・認証・guardに関わる変更は、正常系の成功だけでは完了としない。
