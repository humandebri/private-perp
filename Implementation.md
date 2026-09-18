# private-perp 実装計画

- 版: v0.5（UI基盤実装・本番未承認）
- 最終更新: 2026-09-18
- 対象範囲: 実装アーキテクチャ、リポジトリ構成、永続化設計、注文パイプライン、クライアント設計、検証計画、タスク分解
- 対象外: 画面仕様、文言、法務判断

`Plan.md` が「何を作るか」「誰が何をできるか」を定めるのに対し、この文書は「どう作るか」を定める。両者が衝突する場合、権限モデルと不変条件（`Plan.md` 3.2）は `Plan.md` を優先する。実装上の都合で不変条件を曲げない。

v0.5は `Plan.md` v0.9のB（機密資金層＋ユーザー別HL口座）を実装ベースラインとする。16章の確定仕様に沿ってローカル・testnet実装を開始できる。資金安全性・プライバシー・機密基盤・ガバナンス・法務の本番ゲートは未充足であり、実資金受付やデプロイの承認ではない。frontendは合成デモ、ICPバックエンドは未実装。

---

## 0. 決定の記録（追加分）

`Plan.md` の D1〜D8 に加えて、実装方針として次を決定した。

| # | 論点 | 決定 | 影響 |
|---|---|---|---|
| D9 | 署名経路 | 注文はtrading_coreのAgent、資金移動・Agent承認はfunds_vaultのmaster。ブラウザEOAはログイン・預入・出金意図を署名 | 資金鍵をtrading_coreへ渡さず、任意digest署名APIも提供しない |
| D10 | trading_coreの配置 | Confidential Subnetを維持（D7維持）。遅延を受容する | 署名は毎回 `pzp6e` へのクロスネット呼び出しになる。2章で構造を分析する |
| D11 | リアルタイム経路 | 公開市況だけブラウザ↔HL WS直結。本人データはCanisterの認可済み暗号化ポーリング | ブラウザIPと取引口座をユーザー系WSで直接結び付けない。HL照合コストと遅延を測る |
| D12 | 永続化 | `ic-sqlite-vfs` のみ。`StableBTreeMap` は使わない | 実装が単純になる一方、若い依存に全状態を預ける。4.9で緩和策を定める |

D9 と D10 は組み合わせると「全注文がクロスネット署名待ちになる」ことを意味する。これが本計画で最も強い制約であり、2章で扱う。

D9によりブラウザはHL注文やAgent承認を署名しない。接続EOAの認証、本人HL口座からの預入、出金意図の承認にだけウォレット署名を使う。

署名場所、通信経路、資金経路は別の設計判断である。「Privacyを提供するためCanister署名しか選べない」とはしない。今回はBの資金・鍵管理に合わせてD9を確定したが、署名をCanisterへ集約しても公開送金のリンクは消えない。

---

## 1. 機密資金層＋ユーザー別HL口座のアーキテクチャ

### 1.1 経路図

```
[ユーザーのブラウザ]
  │
  ├─(A) 公開市況だけ ──WS直結──▶ Hyperliquid
  │       wss://api.hyperliquid.xyz/ws         (D11)
  │
  ├─(B) 認証・出金意図 ──EOA署名＋暗号化──▶ funds_vault
  │        └─ 預入は本人HL口座から共通保管口座への本人署名送金
  │
  └─(C) 注文・取消・本人データ ──認証＋暗号化──▶ [trading_core @ Confidential Subnet]
                                                     │
                                                     ├─ 認可・上限・期限（同期）
                                                     ├─ ic-sqlite-vfs（同期トランザクション）
                                                     ├─ await sign_with_ecdsa ──xnet──▶ [pzp6e]
                                                     ├─ await 非replicated HTTPS outcall ──▶ Hyperliquid
                                                     └─ await /info 照合 ──▶ Hyperliquid
```

### 1.2 経路ごとの責務

| 経路 | 何が流れるか | 誰が検証するか |
|---|---|---|
| (A) | 公開市況のみ | ブラウザ。リスク判断の権威にはしない |
| (B) | 認証・資金要求・master action・資金照合 | funds_vault、Hyperliquid。Agent承認もfunds_vaultが実行 |
| (C) | 注文意図、署名済みaction、照合結果、暗号化した本人状態 | trading_core |

### 1.3 Canisterに通さないもの

- 市況データ。中継すると、レプリケートされたHTTPS outcallとコンセンサスを毎秒回すことになる。7ノード分のoutcall費用とコンセンサス負荷が乗り、レイテンシもHL純正より悪化する。
- サービス外のブリッジ。本人HL口座にUSDCを用意する操作はユーザーが外部で行う。

本人データと資金移動はCanisterを通す。市況中継を省くことと、口座情報をブラウザから直接照会しないことを両立させる。

### 1.4 ICPの役割と設計上の境界

BではICPが顧客資金のmaster署名・非公開台帳・Agent署名・復旧を担う。署名APIを呼べるCanisterと変更権限のリスクは残る。鍵の分散だけで非カストディや運営者からの機密性を証明したことにはならない。

- 市況だけHLへ直接接続する。本人データはD11、注文・取消はD9の経路を使う。
- HL標準のSL/TPはHLのtrigger注文を使う。Canisterで価格を監視して同じ仕組みを作り直さない。ユーザー不在時の独自戦略だけが別の実行エンジンを要する。
- フロントエンド配信はICPが必須ではない。
- Bは採用済みだが公開の資金移動は残る。相関耐性はPlan 16.6で評価し、Canister保管だけでD13達成としない。
- Canisterのリスク制限は本サービスの注文にだけ適用される。ユーザー自身・他Agentの直接取引がある口座全体を、ローカルDBだけで厳密に制限することはできない。

### 1.5 機密資金層の実装境界

初期の資金経路はPlan 16.2のHyperCore USDC往復とする。全員のHLポジションを単一口座へまとめず、約定・証拠金・清算はHL標準に委ねる。

- 機密資金層の共通TreasuryとHL取引口座は別の構成要素である。資金層の採用からomnibus取引を導かない。
- NEARのTreasury/FAR/IMTは公式仕様で確認できるが、near.com perpsの鍵管理・口座割当は推測である。参考構造を実証済み実装としてコピーしない。
- 資金層のmaster鍵はfunds_vault、Agent鍵はtrading_coreが保持する。ユーザー単独の資金回収は保証しない。
- 注文DBと資金DBを別Canisterに置く。資金移動の受付・予約・外部実行・照合を14章の状態機械で分離する。
- 入出金、Agent承認、口座別WS、API応答、cloid、監査ログを横断してリンクを調べる。金額・時刻の相関への完全耐性は必須としないが、直接の公開リンクを推測リスクと混同しない。

Plan 16章と本書14章の境界で、資金台帳とmaster actionをtestnet向けに実装する。従来のAgent-only工期はBの見積りに流用しない。

---

## 2. レイテンシ設計

### 2.1 何が遅いのか

`Plan.md` で「HFTは対象外」としたのは性能目標の話だったが、D9/D10により**通常の手動注文のUXに直接効く**問題になった。構造は次のとおり。

| 区間 | 内容 | 既知の値 |
|---|---|---|
| ブラウザ → 境界ノード → subnet | ingress | 数百ms |
| 認可・リスク検証 | 同期・純粋 | 無視できる |
| sign_with_ecdsa | クロスネット（confidential → pzp6e） | **未実測** |
| HTTPS outcall POST | 非replicated、TLS | 数百ms |
| 照合 `/info` | 非replicated POST | 数百ms |

DFINITYのエンジニアは「`sign_with_ecdsa` を呼ぶcanisterを署名subnet上に置けばクロスネット遅延を完全に避けられる」と回答している。本番鍵 `key_1` は fiduciary signing subnet `pzp6e`（34ノード）にのみ配備され、テスト鍵は `fuqsr` にある。**D10の構成ではこの回避策を取れない**ため、クロスネット分の遅延を毎回支払う。

参照: [Sign with ECDSA takes 12+ seconds](https://forum.dfinity.org/t/sign-with-ecdsa-takes-12-seconds-and-costs-0-03/58325)、[Chain-key Signing Performance Improvements](https://forum.dfinity.org/t/chain-key-signing-performance-improvements/64672)

### 2.2 署名subnetの容量制約

- `pzp6e` のtECDSA最大スループットは**サブネット全体で約3.5 sig/s**（pre-signature 100枚が用意されている場合）。これは全ICPアプリで共有される。
- 問い合わせキュー `ecdsa:Secp256k1:key_1` の `max_queue_size` は20。pre-signatureが十分にあれば動的に最大100並行まで受け付ける。溢れると**署名要求が拒否される**。
- 現時点でcanister単位の署名レート制限は無い（DFINITY、2026-05）。
- 署名単価は約26.15B cycles（約$0.035）。値下げは議論中だが未実施。

帰結。**1注文1署名の設計は、サブネット共有資源の上で動く。** バースト時はキュー溢れを前提に、失敗を注文の失敗にせず再試行に回す設計が必須（5.5）。バッチ化（1 actionに複数注文）は費用だけでなく署名スロットの節約でもある。

### 2.3 受付と外部実行を分離する

`submit_order` はcallerとHL口座の所有権、eligibility、入力サイズ、メタデータ、リスク予約を検証する。同じ同期トランザクションでユーザー単位のrequest ID、本文fingerprint、cloid、注文意図を保存し、受付結果を返す。受付はHLの受理でも約定でもない。`raw_rand` は非同期なので同期受付処理に混ぜない。事前補充した安全な乱数からcloidを割り当て、枯渇時は受付を拒否する。

バックグラウンド処理は次の順序を守る。

1. queuedの注文を取得し、同一口座・Agent世代・network・grouping等の互換な注文だけをactionへまとめる。nonceと不変の署名対象を永続化する。
2. worker epochを取得してsigningへ進み、署名を要求する。
3. callbackでepoch・状態を比較し、取消要求、Agent世代、有効期限、kill-switch、policyの鮮度、メタデータ、リスク予約を再検証する。失効したworkerの結果は破棄する。
4. 署名と正確な送信payloadを保存してsignedへ進む。
5. 送信直前にも再検証し、同じICメッセージでCASによりdispatchingと送信意図を保存してからPOSTを発行する。この間に別のawaitを置かない。
6. 応答は照合の手掛かりとして保存する。dispatching以降は照合専用とし、タイムアウトやcallback trapを未送信扱いに戻さない。
7. HLでactionの結果と各注文のライフサイクルを照合する。cancelも独立した署名actionとして処理する。

sweepは件数・cycles・API予算を制限する。queued/signing/signedはepochを更新して回収できるが、dispatching/unknownは照合のみ。永続状態が正本であり、spawnやtimerの継続を正しさの前提にしない。

### 2.4 Phase 1のGo/No-Goゲート（数値）

Confidential Subnet（re2t4）上から実測し、次で判定する。

| 実測 p95（受付→HL受理確認） | 判定 |
|---|---|
| 2秒未満 | 手動注文として許容。Market注文を既定で有効化 |
| 2〜5秒 | Limit注文を主導線にする。Market注文はスリッページ警告を必須にする |
| 5秒超 | 一般利用向けUXは不合格。機密性を自動で弱めず、testnetを指値主体に限定して原因・処理能力を再設計する |

「未実測」の項目を実測せずにPhase 2へ進まない。ここが本計画で最初のGo/No-Goである。

### 2.5 秘匿性に関する補足

クロスネット署名ではactionの平文ではなく32バイトのダイジェストを渡す。ただし署名要求にはkey IDやderivation pathなどのメタデータも含まれるため、「ダイジェストのみ」「何も関連付けられない」とは主張しない。注文本文はクライアント、復号するCanister、執行先のHLで扱われる。D10はCanister内の処理を保護するもので、HL上の注文・ポジションまで隠すものではない。

HTTPS outcallの本文は送信先HLへ開示される。replica・adapter・proxy・TLS終端のどこで本文を扱うかを確認し、全経路がTEE内にあると未検証のまま仮定しない（`Plan.md` 8.3.3）。また、注文をCanister経由にしても経路(A)/(B)からのIP露出は残る。

---

## 3. リポジトリ構成

### 3.1 workspace

```
private-perp/
├── Cargo.toml                    # workspace
├── icp.yaml                      # icp-cli 設定
├── crates/
│   ├── hl-sign/                  # 純粋・非async。署名とaction構築
│   ├── hl-types/                 # 共有型（action, meta, order）
│   ├── db/                       # ic-sqlite-vfs ラッパ。同期のみ
│   ├── policy/                   # policy_registry canister
│   ├── funds-vault/              # master鍵・認証・複式台帳・資金outbox
│   ├── control-guard/            # SNS経由の変更予約・7日猶予
│   └── trading-core/             # trading_core canister
├── frontend/                     # TanStack Start + React + TypeScript / Workers
├── docs/adr/                     # 採用理由・欠点・再検討条件（6本）
├── research/                     # 調査記録（実装対象外）
├── Plan.md
└── Implementation.md
```

### 3.2 クレートと責務境界

| クレート | 責務 | 依存の制約 |
|---|---|---|
| `hl-sign` | action構築、msgpackエンコード、EIP-712ハッシュ、v復元、数値の正規化 | **`async` を一切含めない。** 純粋関数のみ。テストベクトルを同梱 |
| `hl-types` | Hyperliquidのリクエスト/レスポンス型、`meta` パース | 純粋 |
| `db` | スキーマ、Migration、`Db::update` ラッパ、CASヘルパ | **`async` を一切含めない。** `call_perform`/`ic_cdk::call` を含めない |
| `policy` | 国・規約版・検証鍵・緊急停止・allowlist | 小さい。読み取り失敗はfail-closed |
| `trading-core` | 注文認可、状態機械、spawn、sweep、outcall、照合 | 取引Agent署名だけを許す |
| `funds-vault` | 認証、台帳、master署名、出金・配分・照合 | 本人認可を検証。任意hash署名APIは禁止 |
| `control-guard` | 変更予約、7日猶予、許可したupgrade実行 | SNS governanceからの予約だけを許す。顧客情報を保存しない |

`hl-sign` と `db` を非asyncに固定するのは、`ic-sqlite-vfs` の制約（トランザクション内で `await` を跨げない）を**型とCIで守る**ためである。`ic-sqlite-vfs` 本体は `scripts/check-no-await.sh` で `src` 配下の `.await`・`async fn`・`call_perform`・`ic_cdk::call`・`call_raw` を拒否している。同じ検査を `hl-sign` と `db` に適用する。

### 3.3 同期・非同期の境界

- 非asyncクレート: `hl-sign`、`hl-types`、`db`
- asyncを持つクレート: `trading-core`、`funds-vault`、`control-guard`、`policy`
- `trading-core` は `db` の関数を呼ぶとき、必ず「同期ブロックを1つ完結させてから `await` する」順序を守る。
- レビュー時の確認事項: `Db::update` のクロージャから `await` に到達するパスが無いこと。

---

## 4. 永続化（`ic-sqlite-vfs` のみ）

### 4.1 採用バージョン

| 項目 | 値 |
|---|---|
| クレート | `ic-sqlite-vfs` |
| バージョン | `2.0.0` を**完全固定**（`=2.0.0`） |
| 安定レイアウト | v8 |
| feature | `sqlite-precompiled`（Wasmビルド用） |
| リポジトリ | https://github.com/humandebri/ic-sqlite-vfs |

`2.0.0` は破壊的な安定レイアウト変更（v8）である。v6のsegmented page-mapイメージは直接開けない。**import/export/compactはRust facadeにも参照canisterにも公開されていない。** これはrawイメージの標準移行APIの制約であり、SQLによる論理エクスポートが不可能という意味ではない。アプリ固有の整合したバックアップ・復元経路は別途設計・検証する。

### 4.2 MemoryId割り当て

MemoryIdは**デプロイ済みcanisterの寿命の間、変更しない**。255は同梱MemoryManager互換レイアウトが予約しているため、アプリは `0..=254` のみを使う。

| MemoryId | 用途 | canister |
|---|---|---|
| 0 | メインDB（users, agents, orders, order_events, nonces, audit） | trading_core |
| 1 | 予約（将来の独立イメージ。slot catalogに記録する） | trading_core |
| 120 | policy DB | policy_registry |
| 0 | 認証・資金台帳・資金outbox | funds_vault（別Canister） |
| 0 | 変更予約・実行記録 | control_guard（別Canister） |

`MemoryId::new(120)` はic-rusqlite互換の「新品の宛先」慣習に過ぎない。既存のic-rusqliteイメージをこのスロットに向けてはならない。

`Db::init(memory)` を `#[ic_cdk::init]` と `#[ic_cdk::post_upgrade]` の**両方**で、MigrationやDBアクセスの前に呼ぶ。アップグレードに敏感なコードでは `MemoryManager::init_strict` を使い、非空の異物レイアウトを黙って初期化しない。

### 4.3 永続モデルと制約

これは実装前の必須データモデルであり、未検証のCREATE TABLEを完成済みmigrationとして扱わない。数量・価格は正規化十進文字列または範囲検証済み整数とし、浮動小数点で保持しない。

| テーブル | 必須フィールドと制約 |
|---|---|
| users/accounts | user_id、ランダムなaccount_id、HL master、所有権確認時刻、状態。callerとの対応を非公開で保持 |
| agents | account_id、generation、agent_address、derivation_path、承認/失効/期限/照合時刻。UNIQUE(account_id, generation)、UNIQUE(agent_address)。失効世代は再利用禁止 |
| requests | user_id、client_request_id、canonical_body_hash、受付結果への参照、created_at。UNIQUE(user_id, client_request_id)。同じIDで本文が異なる場合は競合エラー |
| actions | action_id、account_id、agent_address、generation、network、nonce、expires_after、canonical_action、署名対象digest、signature、wire_payload、dispatch_state、worker_epoch、lease_until、attempt、policy_version、dispatch_started_at、next_check_at、応答要約。UNIQUE(agent_address, nonce) |
| orders | order_id、request参照、cloid（16バイト・UNIQUE）、銘柄/asset index、方向、kind、価格、元の数量、reduce_only、trigger条件、venue_state、累積約定量、hl_oid、cancel_requested。発注数量を約定数量で上書きしない |
| action_orders | action_id、order_id、操作種別、action内の添字。注文・取消・修正の結果を注文単位で関連付ける |
| nonces | agent_address（PRIMARY KEY）、last_nonce。action生成と同一トランザクションで更新 |
| risk_reservations | account_id、request/action参照、予約内容、状態。受付・解放を原子的に行う。不確定注文の予約を期限だけで解放しない |
| order_events | 注文/action参照、時刻、旧新状態、限定した理由コード。平文payloadを複製しない |
| meta_cache | network/DEX、取得時刻、digest、添字を維持した銘柄/精度/廃止状態の実データ。digestと件数だけでは注文を検証できない |

全参照に外部キー、状態にCHECK、worker/照合対象とユーザー一覧に有界検索用indexを設ける。署名対象・署名・送信payloadは機密データとしてアクセス制御する。署名前はsignatureがNULL、dispatching以降は送信payloadとsignatureが必須という不変条件をDB操作層で検査する。注文種別ごとに必須フィールドを検査し、MarketはHLのスリッページ上限付きIOC指値として構築する。

注文の終端とactionの照合完了を別々に確認してから、定めた保持期間後にpayloadを削除する。未約定・部分約定・unknownを掃除しない。request IDの再実行防止レコードは保持窓を明示し、古い受付要求の拒否と整合させる。

### 4.4 同期トランザクション境界

`Db::update` は短い同期トランザクションとし、awaitや外部callを跨がせない。SQLiteのCOMMITとICメッセージの確定は別である。同じICメッセージがtrapすれば、そのメッセージ内でSQL上コミットした変更も巻き戻る。以前に正常終了したICメッセージの状態と、すでに発行済みの外部作用はcallbackのtrapでは取り消されない。

したがってdispatchingをPOST発行前に記録し、callbackでの状態保存が失敗しても照合へ進める。通常のResult::Errが自動的に書き込みを巻き戻すとは仮定しない。

### 4.5 禁止事項

- **SQLiteの `random()` / `randomblob()` を使わない。** このVFSでは決定的であり、同一呼び出しで同一値になる。cloid・トークン等にはmanagement canisterの非同期 `raw_rand` 由来の安全な乱数を用いる。nonceは乱数ではなく時刻と永続カウンタから割り当てる。
- WAL、`-wal`/`-shm`、mmap、shared-memoryメソッドを使わない（未サポート）。
- `Db::query` は `query_only`/read-only接続である。queryのクロージャ内で状態を書かない。queryで永続化できると仮定しない。
- 接続・文・トランザクションをクロージャの外へ持ち出さない（`SQLITE_THREADSAFE=0`）。
- 任意のSQLを外部入力から組み立てない。値は必ずbindし、識別子を動的生成しない。
- 公開queryで無制限の `LIKE '%...%'`・全表走査・無制限 `ORDER BY` を出さない。一覧は必ず `LIMIT` と決定的なタイブレーカー付きのカーソルページングにする。

### 4.6 CASとfencing

workerは取得時に永続worker_epochを増やす。すべてのawait後の書き込みを `WHERE action_id = ? AND worker_epoch = ? AND dispatch_state = ?` のCASで守り、更新0件なら結果を捨てる。リース期限だけでは、以前の署名callbackが遅れて到着する競合を防げない。

署名前の回収は世代を更新できる。dispatching/unknownの回収は照合workerの再開であり、送信権の再取得ではない。取消・kill-switchは未送信actionのepochを無効化する。送信済みの副作用は無効化できないため、別cancel actionとvenue照合が必要になる。

### 4.7 マイグレーション

- `Migration` のバージョンは厳密に増加させる。`Db::migrate` は `Db::init` の後、fresh-installとpost-upgradeの両方で呼ぶ。
- 各migration本体は「1つのバージョン付きステップ」であり、`IF NOT EXISTS` による冪等初期化として書かない。
- migration SQLは静的に保つ。実行時に組み立てない。
- テーブル再構築・バックフィル・インデックス作成は**本番規模のデータでPocketICで計測**する。1メッセージで終わらない場合は、明示的に再開可能なアプリケーションmigrationに分割する。メッセージを跨ぐSQLiteトランザクションは作らない。
- アップグレード試験は「実際にデプロイされているWasmと安定レイアウト」から「提案するWasm」への経路で行う。スキーマ版・代表データ・整合性・リソースメタデータを検証する。

### 4.8 容量と監視

- 論理DBサイズと、選択した安定メモリのハイウォータを**別々に**監視する。安定メモリは縮小しない。
- v8レイアウトでは、通常のコミットは `db_base_offset` を安定させ、`page_table_bytes` を0に保つ。この2つが増え続ける場合は回帰として扱う。
- `orphan_bytes_estimate` は観測値であり、回収可能性の証明ではない。
- 安定メモリの成長失敗は容量インシデントとして扱う。必要なページ数・上限・cycles・操作サイズを確認せずに盲目的に再試行しない。
- `checksum` は「最後に検証されたチェックサム」であり、コミット境界ではない。通常の書き込みで `checksum_stale` になりうる。
- チェックサムの再計算はcontroller限定のジョブとして `refresh_checksum_chunk` で**分割して最後まで実行**する。大きなDBを1回の公開updateで走査しない。
- 整合性チェックとチェックサム保守のエンドポイントはcontrollerまたは明示的な管理者認可に限定する。

### 4.9 復旧の前提

DBは単なるHLの索引ではない。受付済み未送信の意図、署名済みoutbox、nonce、worker世代、所有権対応、リスク予約はHLから完全復元できない。バージョンとlockfileを固定し、次を本番移行ゲートにする。

- 機密データへの認可・暗号化・整合した時点・サイズ上限を備える論理バックアップ/復元方式を検証する。raw stable-memoryコピーをサポート済みlive backupと呼ばない。
- 古いバックアップを戻しただけで送信を再開しない。未確定actionを照合し、状態の欠落で安全を証明できない場合は停止する。
- nonce欠落時に「十分未来のnonce」を選んで復旧しない。有効窓があり、古い署名の再受理リスクもある。funds_vaultが旧Agentを失効させ、新世代を承認した上で口座を再照合する。master署名の資金outbox欠落はAgent変更では解決しないため、別途送金履歴・予約を照合して停止解除を判断する。
- Agent再生成は公開user_idではなく保存したopaque account_idとgenerationに基づく。対応データの喪失は鍵の自動復元を保証しない。
- レイアウト変更はデータ移行として検証し、旧Canisterと唯一の原本を保持する。

## 5. 注文パイプライン

### 5.1 actionと注文の状態を分ける

actionの状態は `queued → signing → signed → dispatching → reconciled / unknown` とする。未送信が保証できるものだけを `aborted` にできる。`reconciled` は各子操作の結果を照合した意味であり、約定完了ではない。

注文側は `pending / open / partially_filled / filled / cancelled / rejected / unknown` を区別し、HLの状態と累積約定量を保存する。バッチ全体のHTTP成功を子注文すべての成功と解釈しない。取消要求も約定との競合があるため、取消済みとは別に保持する。

### 5.2 冪等性

- `UNIQUE(user_id, client_request_id)` と正規化本文のfingerprintで受付再送を識別する。同一ID・同一内容なら同じ結果、異なる内容なら拒否する。
- nonceは署名者単位で `max(now_ms, last_nonce + 1)` を永続確保し、HL有効窓を検証する。1つのactionに1つ割り当て、子注文ごとには割り当てない。
- 署名再試行は未送信の同一action/digestに限定する。dispatching以降に新nonceや新cloidで自動再注文しない。
- cloidは照合キーであり、永久のexactly-once保証ではない。nonceの記録保持とAgentの寿命に依存するため、失効・期限切れした鍵を再承認しない。

### 5.3 失敗・期限・取消

- 署名拒否は未送信の範囲で有界バックオフする。epoch・期限・policyを再検証する。
- POSTのタイムアウト、応答解釈不能、callback trap、dispatchingでのupgradeはすべて結果不明として照合する。
- `orderStatus` が見つからないだけでは未実行を証明しない。保持期間や可視化遅延を考慮し、未解決ならunknownを維持してユーザーへ提示する。
- `expiresAfter` はactionの受付期限であり、板に残る注文の取消期限ではない。ローカルdeadline超過でcancelled/expiredにしない。
- 未送信取消はepoch無効化とabortedで完結できる。送信可能性のある注文はHL cancel actionを発行し、その結果・約定量を照合する。
- kill-switchの新規停止、HL標準scheduleCancelによる未約定注文の取消、建玉の決済は別操作。停止やdead-man's switchで建玉まで自動解消したと表示しない。
- 結果不明が解消できなければ自動再送せず、予約を保持して安全側に停止する。

### 5.4 HTTPS outcallと照合の信頼

状態変更POSTは非replicated outcallを候補とし、選択したCDK・subnetで動作を検証する。単一送信でも外部効果の原子性は得られない。応答サイズ上限は抽出後JSONではなくraw本文とヘッダーを考慮して設定する。料金は採用APIのcost見積もりと実測で決め、replicatedの式を非replicatedにそのまま適用しない。

非replicatedの読取結果は独立したコンセンサス証明ではない。リスク上限や残高の権威ある入力に採用する場合は応答改ざん・staleを含む信頼モデルを確定する。必要な照合強度が未検証の間は本番を止める。transformで注文別の結果や照合識別子を消さない。本文・署名をログへ出さない。

### 5.5 バッチと優先順位

同一口座・署名Agent世代・network・vault・grouping・builder設定・期限方針が互換な注文だけを、件数とpayloadサイズの上限内でまとめる。異なるユーザーの注文を1署名へ混ぜない。バッチ化の待ち時間にも上限を設ける。Cancel Allも件数上限次第では複数actionになる。取消を新規注文より優先する。

## 6. クライアント設計

### 6.1 公開市況だけをHyperliquid WSへ直結（D11）

ブラウザは `wss://api.hyperliquid.xyz/ws`（mainnet）/ `wss://api.hyperliquid-testnet.xyz/ws`（testnet）へ直接接続する。Canisterを経由しない。

| 用途 | チャネル |
|---|---|
| 全銘柄の中間価格 | `allMids` |
| 板 | `l2Book`（`nSigFigs`・`mantissa`、`fast` で5段） |
| 約定 | `trades` |
| ローソク | `candle`（1m〜1M） |
| 最良気配 | `bbo` |
| 公開銘柄コンテキスト | `activeAssetCtx` |

本人の注文・約定・口座状態・資金履歴は認証・暗号化したCanister APIから取得する。ブラウザでユーザー系HLチャネルを購読しない。ブラウザのnetwork検査で取引口座アドレスの送信がないことを試験する。

Canister側照合は取引口座master addressを使い、Agent addressと取り違えない。初期の本人データはポーリングであり、永続WS中継サーバーは追加しない。

### 6.2 接続とレート制限

HyperliquidのIP単位の制限。

| 制限 | 値 |
|---|---|
| WS接続数 | 10 / IP |
| 新規WS接続 | 30 / 分 |
| サブスクリプション数 | 1,000 |
| **ユーザー系サブスクのユニークユーザー数** | **10 / IP** |
| 送信メッセージ | 2,000 / 分（全接続合計） |
| REST | 1,200 weight / 分（`l2Book`・`allMids`・`clearinghouseState`・`orderStatus` は weight 2） |

クライアント側の設計上の帰結。

- **タブごとに接続を作らない。** 複数タブで1つの接続を共有する（`BroadcastChannel` + leader election）。10接続/IPは複数タブで容易に尽きる。
- 60秒間サーバーからメッセージが無いと切断される。`{"method":"ping"}` を定期的に送る。
- 再接続時はスナップショット（`isSnapshot: true`）を検出して状態を置き換える。差分として適用しない。
- 本人データはCanister照合結果のrevisionと観測時刻で更新する。キャッシュ表示と最新のHL状態を区別し、公開市況から約定を推測して確定しない。
- チャネルは必要になった時点で購読し、画面を離れたら解除する。1,000サブスクは全銘柄で容易に超える。
- 履歴ローソクは5,000本が上限である。長い履歴が要る画面は自前で保持するか、外部ソースを使う。

### 6.3 pending表示（D9のUX補償）

D9とD10により、注文がHLに届くまでに秒単位の遅延がある。ブラウザはCanisterから確認済み状態を取得する。受付とHL受理を区別して表示する。

1. ユーザーが送信 → ブラウザは即座にローカルの `pending` 行を注文一覧に出す（`client_request_id` をキーにする）。
2. `submit_order` の受付応答（`queued`）で `cloid` と `order_id` を受け取り、pending行に紐づける。
3. 暗号化した本人データのポーリングでHL受理の照合結果を取得し、request ID/cloidに対応するpending行を更新する。受付完了を約定と表示しない。
4. `submit_order` の受付が拒否された場合はpending行を取り消し、理由を表示する。
5. 一定時間（例: 15秒）届かない場合は「送信状況を確認中」に遷移し、取消要求の導線を出す。ただし送信前の中止とHL上の取消を分け、確認前は取消済みと表示しない。

「送信したように見せる」のではなく「受付済みで送信中であることを正しく表示する」。ここを偽ると、実際には通っていない注文を約定済みに見せる事故になる。

### 6.4 OSSスタックとライセンス

| 層 | 採用 | ライセンス | 根拠 |
|---|---|---|---|
| 注文の構築・HL型・WS/REST | `@nktkas/hyperliquid` | MIT | 署名・`approveAgent`・31サブスク・batch注文を網羅。TS |
| チャート | `lightweight-charts` | Apache-2.0 | 商用クローズド可。`Plan.md` の性能要件に十分 |
| UI基盤 | `shadcn/ui` + Tailwind | MIT | 一般的 |
| 表 | TanStack Table | MIT | ポジション・注文一覧 |
| 先行実装の参照 | `vipineth/hypeterminal` | MIT | 板・注文チケット・WS信頼性の実装を参照 |
| Rust側の参照 | `infinitefield/hypersdk` | MPL-2.0 | MPLファイルを改変しなければクローズド可。参照のみ |

**採用しないもの。**

| 対象 | 理由 |
|---|---|
| TradingView Advanced Charts / Trading Platform | 今回不採用。企業向け公開サービスとソース非公開は別条件。採用する場合は公開形態・attribution・ライセンスを確認する |
| `kline-orderbook-chart` | 商用プロプライエタリ。license feeが必要 |
| `suenot/profitmaker` | MIT + Commons Clause。「Sell the Software」を禁止 |
| GPL-3.0 / AGPL-3.0 のプロジェクト（freqtrade、freqUI、OctoBot、nofx） | コピーレフト。特にAGPLはネットワーク条項がホスト型サービスと衝突する |
| 無ライセンスのリポジトリ | 全ての権利留保。**公式の `hyperliquid-dex/order_book_server` も無ライセンス** |
| `nomed/hyperliquid`（npm） | npmメタデータはMITを主張するが、リポジトリのLICENSEが404。法的に不明確 |

チャートはLightweight Chartsを採用する。「Advanced Chartsは商用クローズドでは使えない」という旧記述は撤回する。[公式の比較・提供条件](https://www.tradingview.com/free-charting-libraries/)を基準に、必要になった時点で用途と契約を確認する。

### 6.5 Rust側

- 公式 `hyperliquid-rust-sdk` はMITだが2025-10-21から停滞している。**署名実装は公式SDKに依存せず自前で書き**、公式SDKはテストベクトルの生成元としてのみ使う（`Plan.md` 8.8）。
- `hypersdk`（MPL-2.0、活発）はEIP-712署名とbatch注文を持つが、agent承認の対応が未確認である。参照に留める。

---

## 7. Agent承認フロー

1. Plan 16.1のEOA challengeでセッションを認証する。funds_vaultがランダム口座IDとmaster公開鍵を生成し、本人IDへ束縛する。ユーザーが任意のHL口座アドレスを申告する方式にはしない。
2. 認可されたupdateで新しいAgent世代を作り、management callで公開鍵を取得し保存する。queryは保存済みの本人のアドレスを返すだけにする。
3. funds_vaultが取引口座masterでapproveAgentに署名する。登録済みtrading_core caller、口座、生成世代、導出公開鍵を照合し、任意のAgentアドレスの承認依頼を拒否する。
4. HLの承認状態を独立に照合してactiveにする。要求中に世代・所有権が変わったcallbackは捨てる。
5. builder feeを使うなら、別途master署名によるapproveBuilderFeeと上限確認を行う。Agent承認は手数料承認を兼ねない。

アドレス、用途、権限、期限をUIへ明示する。期限・失効後の再利用は禁止し、opaque account_idとgenerationから新しい鍵を導出する。導出結果は世代ごとにキャッシュする。30日有効・27日目切替をtestnetで検証する。停止・失効要求はCanisterが実行し、ユーザー自身によるHL直接解除・直接出金を保証しない。

## 8. フロントエンドの配信

主配信はTanStack Start＋ReactをCloudflare Workers＋Static Assetsへ載せる。公開ページはSSR、取引・資金・履歴はクライアント描画。本人残高・注文本文・ウォレット署名・口座対応表はSSR、server function、Workersログへ渡さない。実装状態はdocs/implementation-status.mdを参照する。

Workersは取引APIを代理しない。資金・署名・注文状態・本人認可はICPに残す。D1/KV/R2/DO、Hono/Express、独自WS中継を追加しない。現UIは外部通信なしの合成デモであり、実際のHL市況とICPには未接続である。

### 8.1 旧ICP配信の参考値（現在の採用構成ではない）

静的フロントエンドの配信はcycles的にはほぼ無料である。

| 項目 | コスト |
|---|---|
| query call | **無料**（単一ノード、コンセンサスなし） |
| ストレージ | 127,000 cycles/GiB/秒（13ノード）≒ **$0.45/GiB/月**。34ノードは332,153 ≒ $1.18/GiB/月 |
| 応答バイト | 課金項目が存在しない |
| ingress受信 | 1,200,000 cycles/メッセージ + 2,000 cycles/バイト（13ノード） |

上記は静的配信だけの旧参考試算であり、現在の価格保証ではない。本人データの照合・暗号化ポーリング、資金台帳、署名のコストは含まない。公開市況の中継は省くが、B全体の費用はPhase 1で再計測する。

### 8.2 旧ICP配信の制限メモ（Workersへ適用しない）

| 制限 | 値 |
|---|---|
| ingressペイロード | 2 MiB |
| query応答 | 3 MiB |
| update応答 | 2 MiB |
| stable memory | 500 GiB / canister |
| wasm | 100 MiB |
| query実行スレッド | 2 / canister |
| update実行スレッド | 1 / canister |

asset canisterは2 MiBを超えるアップロードを自動でチャンクする。3 MiBを超える応答は認定済み `206 Partial Content` に分割され、ゲートウェイが再構成する。

**実務上の制約は query実行スレッドが2本/canisterであること。** 同時接続ユーザー数が増えたとき、ここが先に詰まる。境界ノードのRPS上限（DFINITYスタッフの発言で約1k rps/client、超過で数分のban）は、1Hzポーリング程度では問題にならない。

### 8.3 境界ノードの信頼について（訂正）

以下はICP配信を検討した際の信頼境界メモであり、現Workers配信の検証ではない。現構成ではCloudflare・配信権限者によるJavaScript変更が信頼点になる。guardの7日猶予はUI配信に適用されない。依存固定、配信権限分離、リリースレビューと本番のCSP/connect-src設計を必須とする。入口の地域制限だけではCanister直接呼出しを制限できない。

前回の説明を訂正する。「query応答はcertified dataで検証されるため境界ノードは偽の応答を作れない」は**主体が誤っていた**。

- ゲートウェイが偽の応答を作って検証側に受理させることはできない（ICPルート鍵に連鎖し、部分署名にはsubnetノードの2/3以上が必要）。
- しかし**ブラウザ経路では検証するのはゲートウェイであり、ブラウザではない**。公式ドキュメントは「ブラウザはIC証明書を自分で検証できないので、その検査を委譲する。どのゲートウェイを選ぶかが信頼の決定そのものである」と述べている。
- ゲートウェイは**検証しないという選択**ができる。`raw` ホスト（`<canister-id>.raw.icp.net`）は証明書を破棄する。
- 自分で検証できるのは、HTTPゲートウェイを介さず**canisterと直接話すクライアント**（agent。`read_state` を使う）だけである。

運用上の帰結。certified応答を自分で検証したい場合、フロントエンドは境界ノード任せにせず、重要な値（残高・ポジション・リスク上限）をagent経由で検証してから表示する。自己ホストのゲートウェイも可能である（`dfinity/ic-gateway`、Apache-2.0、活発に保守）。自己ホストは「どのゲートウェイを信頼するか」を自分で決めるという意味であり、経路からゲートウェイが消えるわけではない。

---

## 9. 検証計画

### 9.1 署名のテストベクトル（最初にやる）

`hl-sign` は次を満たさなければ先へ進めない。

1. 公式SDKと同一入力のdigest・符号化が一致する。固定秘密鍵を使う決定的なローカルベクトルと、tECDSAの実署名検証を分ける。tECDSAでは署名の検証と復元アドレス一致を要求し、r/sのバイト一致は要求しない。
2. actionのフィールド順・msgpackの整数表現・十進の正規化が一致する。
3. agent署名時のラッピング（phantom agent相当）とEIP-712 domain/typeが一致する。
4. tECDSA署名から `v` を復元できる（threshold署名の応答に `v` は含まれないため、候補を試して公開鍵と一致するものを選ぶ）。
5. 上記を回帰テストとして固定し、常時実行する。

テストベクトルはリポジトリに固定し、生成元のSDKバージョンを記録する。

### 9.2 PocketIC

- fresh-install、update/query、失敗時のロールバック、アップグレードの4系統。
- 空・代表・上限近傍・不正入力・trap・migration失敗・post-upgradeの各ケース。
- trading_coreに出金署名・資金鍵がなく、funds_vaultの出金actionが本人認可・残高・宛先・一回性を必須とすることを試験する。
- `universe` 全件を走査するasset indexのコンフォーマンステスト。廃止銘柄・未知添字・allowlist外の拒否を含む。
- 受付再送の冪等性、dispatching以降の自動再送禁止、Agent世代の非再利用を検証する。
- POST応答を破棄した状態からの照合による復元。

### 9.2.1 プライバシーと資金権限の検証

- 接続用ウォレットからHL口座まで、送金・承認・API・cloidの公開情報だけで直接辿れる経路を列挙する。Aでは直接送金のリンクが残ることを結果に記録し、D13の達成と取り違えない。
- 本人以外が注文・約定・cloid・口座対応を取得できないことを確認する。queryを公開しないだけでなく、応答・ログ・履歴へのアクセス制御を検証する。
- 口座別WSとAgent承認のブラウザ通信でHLへ渡る情報を確認し、U25の受容範囲と照合する。
- 資金層の本人認可、裏付け、二重仕訳、不確定送金、アップグレード、停止・復旧を試験する。Agent-onlyの「出金コード不在」試験で代用しない。Plan 16.6のA/B0/B1評価も必須とする。

### 9.3 Phase 1で実測する値

| 項目 | なぜ必要か |
|---|---|
| re2t4からの `sign_with_ecdsa` p50/p95 | 2.4のGo/No-Goゲート |
| 受付→HL受理確認 のp50/p95 | 同上 |
| `sign_with_ecdsa` のキュー溢れ発生率 | 再試行設計の妥当性 |
| re2t4でのHTTPS outcallの成否と所要時間 | Confidential Subnet上でのoutcall動作は未回答の公開論点 |
| 7ノードsubnetの課金基準 | re2t4が13ノード基準で課金されるか未回答 |
| nonceの有効窓と衝突時の挙動 | `Plan.md` U8 |
| Agentの有効期限の実仕様 | `Plan.md` U7 |
| 本人データの暗号化ポーリングとHL照合予算 | 6.1、Plan 16.5 |
| Confidential Subnetでのupgrade・状態復旧 | `Plan.md` 8.3.3の前提条件 |

### 9.4 障害注入

- 署名キュー溢れ、署名エラー、POSTタイムアウト、POSTエラー応答、`/info` 不一致。
- queued/signing/signed/dispatching/unknownでのupgrade、POST直後のcallback trap、リース失効後の旧callback、署名中のkill-switch・取消を注入する。
- unknownのdeadline超過でも取消済みにしないこと、部分約定後取消、バッチ内の部分拒否、同じrequest IDで異なる本文の拒否を検証する。
- Hyperliquid API停止中の新規注文停止と、cancel-onlyモードでの取消の成功。
- 安定メモリ成長失敗、`ZeroExtentLimitExceeded`。
- 二重ingress（同一 `client_request_id` の同時送信）。

---

## 10. タスク分解

### 確定仕様の実装（Phase 0〜1）

- Plan 16章の資金経路、master鍵、認証、guardを採用する。資金往復とprivacy評価を注文UIより先に実装する。
- EOA認証、HPKE、複式台帳、資金outbox、照合adapter、master/Agent権限分離を実装する。
- immutable guardの予約・猶予・SNS認可・迂回防止と、停止/unknownからの回復をtest環境で試験する。
- 以下の旧工期は無効。Bの実測後に再見積りする。

### Phase 1（資金往復・署名・privacyスパイク、期間は再見積り）

| # | タスク | 完了条件 |
|---|---|---|
| 1-1 | `hl-sign` の実装（action構築・msgpack・EIP-712・v復元） | 公式SDKとテストベクトル一致 |
| 1-2 | tECDSA署名（re2t4から） | vが復元でき、testnetで注文が通る |
| 1-3 | レイテンシ実測 | 9.3の表の主要項目が埋まる |
| 1-4 | Agent承認・有効期限・失効 | testnetで実測し仕様を確定 |
| 1-5 | cloidによる冪等性 | request IDの冪等性・nonce/Agent寿命の制約を確認 |
| 1-6 | Confidential Subnet上のoutcall・upgrade・復旧 | 動作する |
| 1-7 | `ic-sqlite-vfs` 2.0.0 の疎通 | update/query/upgrade/migrationがPocketICで通る |
| 1-8 | 市況WSと本人データの分離 | ブラウザがHLへ取引口座を送らず、本人状態を暗号化取得できる |
| 1-9 | 共通保管・独立master口座・USDC往復 | 二重計上なく預入から出金まで照合できる |
| 1-10 | guardと資金回復 | 猶予迂回を拒否し、取消・退出・停止時の制約を確認できる |
| 1-11 | A/B0/B1相関評価 | Plan 16.6の成功率と失敗条件を記録できる。未達なら再設計 |

**Go/No-Go**: 署名・資金往復・認可・状態機械の合格が単一ユーザーtestnetへの条件。2.4の性能基準とPlan 16.6のprivacy基準未達は明示し、privacy製品・本番へ進めない。

### Phase 2（single-user testnet MVP、期間は再見積り）

| # | タスク |
|---|---|
| 2-1 | スキーマとMigrationの確定、PocketICでのアップグレード試験 |
| 2-2 | `submit_order`（受付・検証・CAS・spawn） |
| 2-3 | `process` と `sweep` の実装、障害注入 |
| 2-4 | Market/Limit/Cancel/Cancel All/Close、SL/TP（groupingの確定） |
| 2-5 | クライアント: WS接続共有、pending表示、注文一覧 |
| 2-6 | Agent承認フロー（導出・承認・照合・期限表示） |
| 2-7 | 出金本人認可、資金台帳、二重出金・異なるユーザーへの流用拒否の試験 |
| 2-8 | cancel-onlyモード、dead-man's switch、緊急全取消 |

### Phase 3（multi-user testnet closed beta、期間は再見積り）

| # | タスク |
|---|---|
| 3-1 | ユーザー別derivation path、鍵導出のキャッシュ |
| 3-2 | 注文内容の暗号化方式の実装（`Plan.md` U5で決定済みの方式） |
| 3-3 | レート制限、ポジション上限、銘柄allowlistと流動性基準（`Plan.md` 6.3.1） |
| 3-4 | `universe` 全件のコンフォーマンステスト、廃止銘柄の建玉導線 |
| 3-5 | eligibility tokenの検証 |
| 3-6 | builder feeの実装と採算の実測 |
| 3-7 | 監査ログ、平文が出ないことの検査 |
| 3-8 | 照合のREST weight予算の実装と検証 |

---

## 11. REST weight 予算（見落としやすい制約）

Canister自身がHyperliquidのRESTを叩く。制限は**IP単位で1,200 weight/分**で、`clearinghouseState`・`orderStatus`・`l2Book`・`allMids` は weight 2である。

概算。

| 用途 | weight | 件数/分の上限 |
|---|---|---|
| 注文1件の照合（`orderStatus` 2回） | 4 | 300 注文/分 |
| 全ユーザーの状態ポーリング（`clearinghouseState`） | 2/ユーザー | 600 ユーザー/分 |

つまり**この頻度で全ユーザーを一律にポーリングする設計は予算を超える**。1,000ユーザーを30秒ごとにポーリングすると 2,000リクエスト/分 × 2 = 4,000 weight/分で、上限の3倍を超える。

したがってイベント通知と、稼働口座・open/unknown注文への有界な定期照合を併用する。

- 注文は送信直後、unknown解決時、open/部分約定の追跡時に照合する。バックオフと共有予算で負荷を制限する。
- ポジション再同期はセッション開始、注文照合、稼働口座の有界タイマーで行う。ブラウザの`notify_fill`は初期実装しない。公開市況WSを見ても本人の約定は確定できない。
- 全件走査は低頻度・分割とするが、稼働口座を1日古い状態のままリスク判断しない。ブラウザ通知は認可・レート制限されたヒントに限定し、通知がなくても照合を継続する。
- 状態が古い場合はリスクを増やす注文を停止する。受付時の予約によりサービス内の並行注文を数え、他Agentや直接取引を含む口座全体の厳密な上限保証とは区別する。

この予算はcanisterのIPがどう数えられるかに依存する。replicated outcallは各ノードが個別に送るため、subnetのノード数だけIPが分かれる可能性がある。**正確な帰属はPhase 1で実測する。** 実測までは保守的に「1つの予算を共有する」として設計する。

---

## 12. 実装判断と実測待ち事項

| # | 決定・残件 | 次の確認時点 |
|---|---|---|
| U16 | HPKE採用、Plan 16.5。実装ライブラリと鍵認証を試験する | 選択済み、Phase 1検証 |
| U17 | 5秒超は一般UX不合格。機密性を自動降格せずtestnet指値主体で改善 | 決定済み |
| U18 | 建玉単位のpositionTpsl、reduce-only。複雑なbracketは後回し | 決定済み |
| U19 | 公開市況WSをBroadcastChannel＋leader electionで共有 | 決定済み |
| U20 | REST weightのIP帰属（subnetのノードごとか、集約されるか） | Phase 1 |
| U21 | =2.0.0/v8固定。更新は実データmigration試験を必須とし、自動追従しない。新Canisterは鍵IDも変えるため単純移設しない | 方針決定、各更新で検証 |
| U22 | notify_fillは不採用。Canisterの有界定期照合＋注文/セッションイベント | 決定済み |

資金経路・鍵・口座別通信はPlan 16章で確定した。U20等の外部環境の実測値は未確認のまま数字を埋めない。

---

## 13. 参考資料

- [Hyperliquid nonceとAgentの失効・再利用制約](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/nonces-and-api-wallets)
- [Exchange endpoint：expiresAfter・取消・builder承認](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint)

`Plan.md` 15章に加えて。

- [IC HTTPS interface（5エンドポイント。WebSocketなし）](https://docs.internetcomputer.org/references/ic-interface-spec/https-interface/)
- [IC エッジインフラ（ブラウザ→ゲートウェイ→境界ノード→replica）](https://docs.internetcomputer.org/concepts/edge-infrastructure/)
- [静的サイトの仕組み（「どのゲートウェイを選ぶかが信頼の決定」）](https://docs.internetcomputer.org/guides/frontends/static-site/how-it-works/)
- [Certification（raw ホストは証明書を破棄する）](https://docs.internetcomputer.org/guides/frontends/certification/)
- [Canister migration（canister IDを保つfull migrationでtECDSA鍵が維持される）](https://docs.internetcomputer.org/guides/canister-management/canister-migration/)
- [IC Resource limits](https://docs.internetcomputer.org/references/resource-limits/)
- [Hyperliquid WebSocket](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket) / [Subscriptions](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/subscriptions) / [Timeouts and heartbeats](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/timeouts-and-heartbeats) / [Rate limits](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/rate-limits-and-user-limits)
- [ic-sqlite-vfs API安定性契約](https://github.com/humandebri/ic-sqlite-vfs/blob/main/docs/API_STABILITY.md) / [運用](https://github.com/humandebri/ic-sqlite-vfs/blob/main/docs/OPERATIONS.md)
- 調査記録: `research/icp-realtime-static-frontend.md`、`research/hyperliquid-oss-research.md`

### 未検証として残した項目

- ユーザー系WSサブスクに署名が不要であること（スキーマからの推論。公式の明示文なし）
- `pzp6e` の3.5 sig/s は2026-02時点のDFINITY発表値。現在値は変動しうる
- Confidential Subnet（re2t4）の現在のノード数・稼働状況と課金基準
- HyperliquidのAPI利用規約・商標ポリシー（公式ドキュメント索引からは見つからなかった）
- 公式SDKの `approveAgent`・batch注文の対応範囲（Rust版は未確認）

---

## 14. 資金層・認証・guardの実装契約

### 14.1 funds_vaultの永続データ

各Canisterは別DBを持つ。funds_vaultのMemoryId 0を注文DBと混同しない。VFSのversion/layoutを固定し、4章と同じinit/post_upgrade・同期トランザクション規則を適用する。WALや通常ファイルシステムのbackup APIを前提にしない。現時点ではCargo.lockも実装もなく、依存の実APIは最初のコンパイル時に固定版ソースと照合する。

| テーブル群 | 必須の制約 |
|---|---|
| identities / sessions / challenges | EOAとランダムuser_idの一意対応。challenge nonce一回性、Principal・origin・用途・期限・失効を検証 |
| custody_accounts | reserve/tradingの用途、opaque account_id、導出path、master address、network。reserveに取引Agentを承認しない |
| journals / postings | journalごとに借方貸方が同額、資産・単位一致。整数overflow拒否。外部イベント・要求からの重複仕訳を一意制約で拒否 |
| fund_requests / reservations | 本人、request ID、本文hash、金額、確定宛先、EOA intent署名、期限。残高不足・二重拘束・他人への付替えを拒否 |
| fund_actions / master_nonces | canonical action、digest、署名、wire payload、dispatch state、epoch、lease、照合予定。masterごとのnonceを同一同期コミットで割当て |
| external_events / reconciliation | 外部の安定ID、network、口座、相手先、資産、金額、時刻、種別、証拠参照。公開APIの欠落・競合はunknownとして保持 |
| key_registry / audit | HPKE key ID・期限、Agent世代承認、限定した理由コード。平文intentや対応表を公開ログへ出さない |

残高は仕訳から導けるようにし、キャッシュ残高を更新する場合は仕訳と同一トランザクションで更新する。未配分資産と取引口座equityは別勘定とし、共通reserveの現金を複数ユーザーへ重複配分しない。

### 14.2 資金状態機械と外部照合

資金要求は`accepted → reserved → executing → settled`、または`rejected/unknown`。複数の外部移動を1つの原子的操作と扱わず、配分・回収・払出しを個別のfund_actionへ分ける。各actionは注文と同じ`queued/signing/signed/dispatching/reconciled/unknown/aborted`を使い、dispatching永続化後にだけ外部送信する。

- 入金はブラウザ提示のhashや成功表示では計上しない。HLで宛先、認証済み送金元、資産、金額、安定イベントIDを検証する。新しい入金の額・時刻だけから本人を推定しない。
- 出金予約後、ユーザー別口座の出金可能額を確認して回収し、回収の確定後にreserveから本人へ払う。途中の応答喪失は別送金に置き換えず照合する。共通reserveに資金があっても、未確認の回収や未確定PnLを先払いしない。
- 新規発注と回収を口座ごとの資金移動ロック・世代で調整する。coreはvaultからの認証済み配分状態を確認し、移動中の証拠金を利用可能として数えない。取消・reduce-onlyを不必要に妨げない。
- 非replicated POSTを選び、外部送信はDBのawait外で行う。資金計上に用いる読取はreplicated outcallによる確定イベント照合を初期方針にする。決定的なtransformは意味のある金額・宛先・IDを消さない。不一致や安定IDを取得できないケースは計上しない。
- replicated読取でもHLの嘘・履歴欠落を暗号学的に排除できない。HTTPS/APIの信頼とICP内の合意を区別する。送金ごとの一意な確定根拠が得られることをPhase 1で確認し、得られなければ実資金の実装を有効化しない。
- master nonce/outboxの巻戻しは特に危険である。復元後は送信停止から開始し、外部履歴・全予約・残高を照合する。旧署名の期限とHLの受理条件を確認できない資金actionを自動再署名しない。

### 14.3 API境界

初期インターフェースは以下の操作に限定する。名称は設計上の名前であり、実装済みCandidではない。

- vault: `issue_challenge`、`open_session`、`revoke_session`、`get_funding_instructions`、`request_allocation`、`request_withdrawal`、`get_fund_status`、`request_agent_revocation`。
- core: `submit_order`、`cancel_order`、`cancel_all`、`get_account_snapshot`。closeはreduce-only注文として扱う。
- guard: `schedule_upgrade`、`cancel_upgrade`、`execute_upgrade`、`get_upgrade_status`。予約内容・実行時刻の変更では新しい7日猶予を開始する。

queryでも本人認可を省かない。vaultの認証結果をcoreが利用する場合、登録済みvault callerから配布された期限・失効世代付きセッションだけを受け入れる。ユーザーの入力にある`caller`を信用しない。失効伝達が未確認のセッションは資金要求・新規リスク受付に使わない。Canister間の呼出し元ID、対象口座、用途、世代、request IDを検証し、callbackの再入・古い応答をfencingする。

HPKE鍵は用途別に生成・更新し、暗号化秘密鍵を公開queryへ出さない。受理前の最大payloadは16 KiB、口座あたり未送信注文は100件、資金移動は1件を初期上限とする。上限はDoS試験で調整する。失効鍵で新規受付しないが、受付済みの照合に必要なデータを期限だけで破棄しない。

### 14.4 変更猶予と障害試験

guardは顧客資金を署名しない。対象Canisterの権限拡大、認証鍵・登録Canister・出金規則・危険なpolicy変更も、管理API経由で猶予を迂回できない設計にする。緊急停止は即時でも、解除や制限緩和は記録したSNS経路で行う。SNS自体のupgradeで呼出し主体が悪意を持つ場合にも、guard側の7日猶予は省略できないことを試験する。

合格が必要な失敗試験:

- 偽のEOA、別Principal、期限切れ、challenge再使用、宛先差替え、別networkのintent。
- 入金イベント重複、仕訳不均衡、並行出金、回収中の発注、送金成功後の応答喪失。
- master/Agent取り違え、coreからの任意出金署名依頼、失効世代のcallback。
- guardへの非SNS予約、早期実行、WASM/引数差替え、controller/reinstall/stop/deleteによる迂回。
- cycles不足、HL停止、Canister upgrade、古いDB復元、HPKE鍵更新中の受付・照合。
- 平文ログ、他人のquery、ブラウザからの取引口座送信、公開proposalへの機密情報混入。

これらは今後実装する試験仕様であり、この文書改訂で実行済みにはなっていない。

## 変更履歴

| 版 | 日付 | 変更 |
|---|---|---|
| v0.1 | 2026-09-18 | 初版。D9〜D12を決定し、確定アーキテクチャ、レイテンシ設計、リポジトリ構成、`ic-sqlite-vfs`のみの永続化、注文パイプライン、クライアント設計、検証計画、タスク分解を整理 |
| v0.2 | 2026-09-18 | Plan v0.4と整合。Agent-onlyの実装ベースラインを明示し、機密資金層とHL口座を分離する検討境界・依存実装ゲート・検証項目を追加。D9は維持し、D11のIP/口座露出とtECDSA/outcallの保護範囲を明記 |
| v0.3 | 2026-09-18 | action/注文の分離、送信前永続化、fencing、期限・取消・Agent世代、所有権確認、復旧とリスク照合の設計を修正。資金プライバシーと署名経路の境界を明確化 |
| v0.4 | 2026-09-18 | Plan v0.8のBへ整合。vault master/取引Agent、EOA認証、本人データのCanister経由、資金DB・状態機械、immutable guard、開発と本番のゲートを決定。Agent-onlyの出金禁止・直接退出・旧工期を撤回 |
| v0.5 | 2026-09-18 | Workers主配信とStart＋Reactの合成UI基盤、固定依存・lint・型・テストを追加。ADR 6本を記録。旧ICP配信試算を参考扱いに変更し、TradingViewの過度な断定を撤回。実ICP APIは存在せず接続工程は保留 |
