# 状態遷移：action・資金要求・注文

- 根拠：`Implementation.md` 2.3、4.4、4.6、5章、14.2、`Plan.md` 3.2、16.2
- 状態：設計契約。実装はPhase 1以降

## 1. 3つの状態機械の関係

| 状態機械 | 単位 | 目的 |
|---|---|---|
| action状態 | 署名・送信する1操作 | 署名・送信・照合の責務を分離する |
| 資金要求状態 | ユーザーの1要求（配分・回収・払出し） | 予約・移動中・確定を区別する |
| 注文ライフサイクル | 1注文 | HL上の状態と累積約定量を保持する |

1つの資金要求・注文は、複数のaction（署名・送信の単位）を持ち得る。1つのactionは複数の子注文を持ち得る（バッチ）。**actionの照合完了は約定完了でも出金完了でもない。**

正準の状態名は英語の識別子とする。表示文言は8節の表で固定する。

## 2. action状態

```text
queued ──▶ signing ──▶ signed ──▶ dispatching ──▶ reconciled
   │           │           │            │
   └───────────┴───────────┘            └────────▶ unknown
              aborted（未送信が保証できる場合のみ）
```

| 状態 | 意味 | DB不変条件 |
|---|---|---|
| `queued` | 受付済み・未署名 | `signature IS NULL`、payload未確定、`worker_epoch`あり |
| `signing` | 署名要求中 | `signature IS NULL`、`lease_until`設定、`worker_epoch`増加済み |
| `signed` | 署名済み | `signature`必須、署名対象digest・wire payload保存済み、未送信 |
| `dispatching` | 送信権を取得済み | `signature`と正確な送信payloadが必須。POST前にCASで保存 |
| `reconciled` | 各子操作の結果を照合済み | 子注文ごとの結果と照合根拠を保持 |
| `unknown` | 外部効果が不明 | 予約を解放しない。照合workerの対象として保持 |
| `aborted` | 未送信を保証して取消 | 署名・送信が発生していないactionのみ |

許可遷移:

| From | To | 条件 |
|---|---|---|
| `queued` | `signing` | 互換な注文のみをバッチ化し、nonceと不変の署名対象を同一トランザクションで永続化した |
| `queued` | `aborted` | epoch無効化、kill-switch、受付期限切れ、リスク予約の解放が同一トランザクションで完了した |
| `signing` | `signed` | 署名応答を受領し、epoch・状態・取消要求・Agent世代・期限・kill-switch・policy鮮度・リスク予約を再検証した |
| `signing` | `queued` | 署名拒否で有界バックオフ。epochを更新して再取得させる |
| `signing` | `aborted` | 署名が成立していないことを確認できた |
| `signed` | `dispatching` | 送信直前の再検証後、同じICメッセージ内でCASにより`dispatching`と送信意図を保存した（間に`await`を置かない） |
| `signed` | `aborted` | 取消・kill-switch・期限切れで未送信を保証できた |
| `dispatching` | `reconciled` | HLの結果と各子注文のライフサイクルを照合した |
| `dispatching` | `unknown` | タイムアウト、応答解釈不能、callback trap、dispatching中のupgrade、照合不能 |

禁止遷移と理由:

- `dispatching` → `aborted`。送信済みの外部効果は無効化できない。
- `dispatching`・`unknown` → `queued`/`signing`（自動再署名）。新しいnonce・cloidでの自動再注文を禁止する。
- `unknown` → `rejected`／`cancelled`。時間経過と`orderStatus`不在だけでは未実行を証明できない。
- `reconciled` → 任意状態。照合後に新たな外部効果が必要な場合は新しいactionを作る。

epoch・lease・CAS（`Implementation.md` 4.6）:

- action取得時に永続`worker_epoch`を増やす。
- `await`後の書き込みは `WHERE action_id = ? AND worker_epoch = ? AND dispatch_state = ?` で守り、更新0件なら結果を破棄する。
- リース期限だけでは、遅延した旧署名callbackの競合を防げない。
- 署名前の回収は世代更新で可能。`dispatching`/`unknown`の回収は照合workerの再開であり、送信権の再取得ではない。

## 3. 資金要求状態

```text
accepted ──▶ reserved ──▶ executing ──▶ settled
    │            │             │
    └────────────┴─────────────┴──▶ rejected（成立していないことを確認できた場合）
                                 └──▶ unknown（外部効果が不明）
```

| 状態 | 意味 | 制約 |
|---|---|---|
| `accepted` | 要求を受付し、冪等性キーを確定した | 残高・宛先・本人認可は未検証でもよい。受付は資金移動の開始ではない |
| `reserved` | 残高を拘束し、資金移動nonceと操作IDを永続化した | 二重拘束・他人への付替えを拒否。未確定の収益を収益計上しない |
| `executing` | 対応するactionが`signing`〜`dispatching` | 口座単位の資金移動ロックと世代で、新規発注・回収の順序を調整する |
| `settled` | 外部イベントを照合し、仕訳が確定した | 仕訳の借方貸方が同額。確定根拠の外部イベントIDを保持 |
| `rejected` | 実行されていないと確認できた | 解放は同一トランザクション。応答喪失を理由に`rejected`にしない |
| `unknown` | 外部効果が不明 | 時間経過だけで解放・再送しない。ユーザーへ提示し続ける |

初期資金経路（`Plan.md` 16.2）:

```text
本人HL口座 →(本人署名)→ 共通保管口座 →(master署名)→ ユーザー別HL取引口座
                                    ←(master署名)←
共通保管口座 →(master署名)→ 本人HL口座
```

- 配分・回収・払出しを個別のfund_actionに分ける。複数の外部移動を1つの原子的操作として扱わない。
- 出金予約後、ユーザー別口座の出金可能額を確認して回収し、回収の確定後にreserveから本人へ払う。共通reserveに資金があっても、未確認の回収や未確定PnLを先払いしない。
- 入金はブラウザ提示のhashや成功表示で計上しない。HLで宛先、認証済み送金元、資産、金額、安定イベントIDを検証する。
- 未配分残高・出金予約・移動中資産・ユーザー別equityを別勘定とする。共通保管資産とユーザー別口座資産を二重計上しない。
- 台帳差異または鮮度不足では新規配分とリスク増加を停止する。安全に裏付け・本人認可を確認できる出金まで一律停止しないが、不明な資金を支払わない。

## 4. 注文ライフサイクル

正準状態: `pending` / `open` / `partially_filled` / `filled` / `cancelled` / `rejected` / `unknown`

| 状態 | 意味 | 備考 |
|---|---|---|
| `pending` | ローカル受付（`queued`〜`dispatching`） | ブラウザは`client_request_id`で即時表示する。HLには未到達の可能性がある |
| `open` | HL受理済み・未約定 | `hl_oid`を保持 |
| `partially_filled` | 一部約定 | 累積約定量を保持。発注数量を上書きしない |
| `filled` | 全量約定 | 累積約定量 = 発注数量 |
| `cancelled` | 取消済み | 取消actionの照合根拠を保持 |
| `rejected` | HLが拒否 | 理由コードを保持 |
| `unknown` | 結果不明 | 再発注を促さない |

- バッチ全体のHTTP成功を子注文すべての成功と解釈しない。`action_orders`で子注文ごとに関連付ける。
- `cancel_requested`は状態と別に保持する。取消要求中の約定は`partially_filled`／`filled`として反映し、`cancelled`にしない（取消と約定の競合）。
- Cancel Allも件数上限次第で複数actionになる。取消を新規注文より優先する。
- `expires_after`はactionの受付期限である。ローカルdeadline超過で`cancelled`／`expired`にしない。
- 未約定・部分約定・`unknown`を掃除しない。注文の終端とactionの照合完了を別々に確認してから、定めた保持期間後にpayloadを削除する。

## 5. 照合規則

| 事象 | 扱い |
|---|---|
| POSTタイムアウト | `unknown`。照合workerがHL履歴を確認する |
| 応答は届いたが解釈不能 | `unknown`。生応答を照合の手掛かりとして保存する |
| callback trap | `unknown`。`dispatching`はPOST発行前に永続化済みのため照合へ進める |
| `dispatching`中のupgrade | `unknown`。アップグレード後に照合から再開する |
| `orderStatus`が見つからない | 未実行の証明ではない。保持期間・可視化遅延を考慮し、未解決なら`unknown`を維持する |
| 安定イベントIDが取得できない入金 | 計上しない |
| 非replicated読取の結果 | 単独では資金計上の権威にしない。replicated outcallによる確定イベント照合を初期方針とする |
| 照合で取消済みと判明 | `cancelled`へ遷移し、取消actionの照合根拠を残す |
| 照合で部分約定と判明 | `partially_filled`。リスク予約は約定分だけ消費する |

- sweepは件数・cycles・API予算を制限する。永続状態が正本であり、spawnやtimerの継続を正しさの前提にしない。
- 結果不明が解消できなければ自動再送せず、予約を保持して安全側に停止する。
- 障害時は新規リスク増加停止、照合継続、可能な取消・reduce-only・確認済み出金を優先する。

### 5.1 停止操作の分離

| 操作 | 効果 | 効果がないもの |
|---|---|---|
| kill-switch（新規停止） | 新規のリスク増加を止める | 建玉の自動決済、既存取引所注文の取消 |
| HL標準`scheduleCancel`による未約定注文の取消 | 板に残る注文を取り消す | 保護用SL/TPが消える可能性がある（UIで明示） |
| 建玉の決済 | reduce-only注文を発行する | 自動成行決済はしない |

緊急停止やdead-man's switchで建玉まで自動解消したと表示しない。dead-man's switchは既定OFF。

## 6. fencing・再入・古いcallback

- すべての`await`後の書き込みを`worker_epoch`のCASで守る。
- callbackではepoch・状態・取消要求・Agent世代・有効期限・kill-switch・policy鮮度・メタデータ・リスク予約を再検証する。失効したworkerの結果は破棄する。
- Canister間呼び出しでは、呼出元ID・対象口座・用途・世代・request IDを検証する。
- 失効伝達が未確認のセッションを資金要求・新規リスク受付に使わない。
- 二重ingress（同一`client_request_id`の同時送信）は、`UNIQUE(user_id, client_request_id)`により1件だけが受付を確定する。

## 7. 復旧

- 古いbackupを戻しただけで送信を再開しない。送信停止から開始し、外部履歴・全予約・残高を照合する。
- master nonce/outboxの巻戻しは特に危険である。旧署名の期限とHLの受理条件を確認できない資金actionを自動再署名しない。master署名outboxの欠落はAgent変更では解決しない。
- nonce欠落時に「十分未来のnonce」を選んで復旧しない。`funds_vault`が旧Agentを失効させ、新世代を承認した上で口座を再照合する。
- Agent再生成は公開`user_id`ではなく保存したopaque `account_id`と`generation`に基づく。対応データの喪失は鍵の自動復元を保証しない。
- 状態の欠落で安全を証明できない場合は停止する。

## 8. 表示文言の対応（正準 → UI）

`ui-spec.md` 5節と `Implementation.md` 6.3に基づく。

| 正準 | UI表示 | 現行デモ（`frontend/src/domain/demo.ts`） |
|---|---|---|
| （ローカル送信直後） | 受付確認中 | 未実装（`queued`で代用） |
| `pending`（action `queued`） | 受付済み・送信準備中 | `queued`（「受付済み・送信準備中」） |
| `pending`（action `signed`/`dispatching`） | 送信済み・確認中 | 未実装 |
| `open` | HL受理 | `open`（「HL受理（模擬）」） |
| `partially_filled` | 部分約定 | `partial`（**別名。Phase 2で正準名へ寄せる**） |
| `filled` | 約定 | `filled` |
| `cancelled`（`cancel_requested`中） | 取消確認中 | 未実装 |
| `cancelled` | 取消済み | `cancelled` |
| `rejected` | 拒否 | `rejected` |
| `unknown` | 結果不明（再送しない） | `unknown` |

現行デモはローカルの受付確認中と`pending`を区別せず、取消確認中を持たない。この差はPhase 2で解消する（Phase 0ではコードを変更しない）。

## 9. 未確定事項

| 項目 | 確定時期 |
|---|---|
| バッチ化の互換条件と待ち時間上限 | Phase 1（1-1〜1-5） |
| 照合の周期・バックオフ・共有予算配分 | Phase 1（11章のweight予算実測） |
| `unknown`を解消できない場合の運用手順 | Phase 3（復旧試験と同時） |
| 保持期間の具体値 | Phase 2-1（スキーマ確定時） |
