# 権限表：Canister責務・許可caller・署名可能action

- 根拠：`Plan.md` 3.1〜3.3、8.1、16.1、16.3、`Implementation.md` 1.2、3.2、14.1、14.3
- 状態：設計契約。実装・実機検証は未完了

## 1. 読み方

- 主体は「ユーザー（認証EOA/ブラウザ）」「funds_vault」「trading_core」「policy_registry」「control_guard」「SNS governance」「運営」に分ける。運営は資金権限を持たない別主体として扱う。
- 「署名可能action」は、そのCanisterが保持する鍵で署名できる対象を限定列挙したものである。列挙外の署名要求は実装してはならない。
- 本表は通常コードの権限である。SNSが任意にupgradeできる状態では、可決による資金流出・情報公開が残る（`Plan.md` 3.1末尾）。

## 2. Canister責務

| Canister | 責務 | 持たないもの |
|---|---|---|
| `funds_vault` | EOA認証とセッション発行、共通保管口座とユーザー別取引口座のmaster鍵管理、複式台帳、資金要求・予約・outbox、Agent承認・失効、入出金・配分・回収の照合 | 注文の執行、ユーザーの平文注文内容、任意digest署名API |
| `trading_core` | 注文・取消・決済の認可と状態機械、口座別・世代別Agent鍵、署名actionの構築と送信、HL照合、口座snapshot配信 | master鍵、出金署名、資金台帳の更新権限 |
| `policy_registry` | 国・規約版・検証鍵・緊急停止・allowlistの読み出し | 資金・注文の執行。読み取り失敗はfail-closed |
| `control_guard` | SNS governanceからの変更予約、7日猶予、予約内容と一致するupgradeの実行 | 顧客資金の署名、顧客情報の保存、任意management call |
| `frontend`（Workers配信） | 公開説明のSSR、取引・資金・履歴画面の配信、公開市況のHL直結 | 平文注文・本人残高・ウォレット署名の保持、資金DB、注文API |

## 3. 権限表（通常コード）

| 操作 | ユーザー | trading_core / Agent | funds_vault | 運営 / SNS |
|---|---|---|---|---|
| 注文・取消・決済 | 認可・要求 | 検証して執行 | 担当外 | 裁量取引は不可 |
| 入金計上・HLへの配分 | 入金・配分を認可 | 出金権限なし | 入金確定と残高・配分を管理 | 任意移動は不可 |
| 出金 | 本人が宛先・金額を認可 | 不可 | 残高・拘束を検証して執行 | 通常出金にDAO投票不要 |
| 第三者への資金移動 | 本人残高の出金のみ | 不可 | 有効な認可なしでは不可 | DAO予算としての流用は禁止 |
| Agent承認・解除 | 停止・解除を要求 | 新規署名を停止 | master署名で実行・照合 | ユーザーによるHL直接解除は不可 |
| 新規注文停止・リスク上限引下げ | 自分の取引を停止 | 制限を強制 | 新規配分を制限 | 限定運営権限で可能（停止方向のみ） |
| 出金先・持ち分の変更 | 本人認可が必要 | 不可 | 証跡付き状態遷移のみ | 任意書換えAPIは設けない |
| WASM・controllerの変更 | 公開情報を検証・退出 | 不可 | 不可 | SNS＋guard。upgradeは7日猶予、controller変更は禁止 |

## 4. Canister別の許可caller・保持鍵・署名可能action

| Canister | 許可caller | 保持鍵 | 署名可能action | query |
|---|---|---|---|---|
| `funds_vault` | 認証済みユーザー（update）、登録済み `trading_core`（限定メソッド）、`control_guard`（upgrade実行者として） | 共通保管口座master、ユーザー別取引口座master（tECDSA） | HL `usdSend`等の資金移動、口座のAgent承認・解除、資金移動nonce付きaction | 本人の資金状態のみ |
| `trading_core` | 認証済みユーザー（vault発行のセッション付き）、`funds_vault`（配分状態の問い合わせ） | 口座別・世代別Agent鍵（tECDSA） | 注文・取消・修正・SL/TP等の取引action（Agent署名） | 本人の注文・建玉・snapshotのみ |
| `policy_registry` | 資金・注文系Canister、`control_guard`（更新） | なし | なし | eligibility・規約版・allowlist・停止状態（公開範囲を限定） |
| `control_guard` | SNS governance principal（予約・取消）、誰でも可（予約済み内容の実行トリガ） | なし | なし（署名鍵を持たない） | 対象ID、WASM hash、引数hash、実行可能時刻、状態 |
| `frontend` | 公開 | なし | なし（ブラウザからの署名は本人のEOA、Canisterへの本人データはHPKE） | 公開情報のみSSR |

補足:

- `trading_core` から `funds_vault` へ出金や任意digest署名を要求する経路は作らない。`trading_core` → `funds_vault` の呼び出しは「配分状態の確認」に限定する。
- `control_guard` は署名鍵を持たず、顧客情報を保存しない。
- `policy_registry` の読み取り失敗時は、新規受付・新規リスク増加を停止する（fail-closed）。

## 5. 設計上の不変条件

`Plan.md` 3.2を実装時に検査可能な形で列挙する。

1. 資金と取引の権限分離。`trading_core` は取引Agentのみ。`funds_vault` は呼出元だけでなく、移動の目的・金額・宛先・本人認可・残高を検証する。
2. 顧客資金と運営資金の分離。cycles・開発費・Treasuryへの流用をしない。未確定資金を収益計上しない。
3. 持ち分と裏付けの整合。二重計上・二重出金を禁止し、利用可能額・配分済み資産・証拠金拘束・出金予約・送金中を区別する。不明額を出金可能残高へ足さない。
4. 出金の本人認可。金額・宛先・資産・network・nonce・期限に束縛する。通常出金にDAO投票は不要。
5. 不確定な外部効果の照合。送金前に予約と操作IDを永続化し、応答喪失を未送金とみなさない。
6. 鍵と変更権限。tECDSA鍵はCanister ID・derivation path・key IDに束縛される。同じCanisterの悪意ある新コードも署名できる前提でupgrade権限を監査する。
7. 停止・退出・復旧。取引停止と出金停止を分離し、未検証の自己回収を約束しない。
8. 侵害時の影響範囲。Agent侵害は証拠金の不正取引、資金層または変更権限の侵害は預かり資産全体に及ぶ。TEEは悪意ある正規コードを、SNSは実装バグを防がない。

## 6. caller検証とセッション伝播

`Implementation.md` 14.3に基づく。

- queryでも本人認可を省かない。応答・ログ・履歴へのアクセス制御を本人単位で検査する。
- `trading_core` は、登録済み `funds_vault` から配布された期限・失効世代付きセッションだけを受け入れる。ユーザー入力の `caller` 相当の値を信用しない。
- 失効伝達が未確認のセッションでは、資金要求と新規リスク受付を行わない。
- Canister間呼び出しでは、呼出元ID・対象口座・用途・世代・request IDを検証し、callbackの再入と古い応答をfencingする（`state-machines.md` 6節）。
- HPKE秘密鍵を公開queryへ出さない。鍵は用途別に生成・更新する（`api-contract.md` 5節）。

## 7. 運営者に与えない権限

`Plan.md` 3.3を維持する。

- 顧客資金の任意出金、本人認可のない出金先変更・他口座移動。
- 完全な秘密鍵の取得、任意メッセージへの資金鍵署名。
- 顧客資金のDAO運営予算への転用。
- ユーザーの明示した上限を超える注文。
- 正当な停止・退出要求の恣意的な妨害。
- 平文の注文内容の閲覧。

## 8. 未確定・Phase 1で確定する項目

- `policy_registry` の公開query範囲（eligibilityの粒度）。
- `funds_vault` ↔ `trading_core` 間のセッション伝達方式（認証済み応答の形態、失効伝達の周期）。
- `control_guard` のSNS generic function呼び出し形式と、実行トリガの公開範囲。
- 開発環境で残す開発者controllerの範囲（本番controller除去はPhase 4の別途承認）。
