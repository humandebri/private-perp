# 環境分離：local・testnet・mainnet

- 根拠：`Plan.md` 16.1、16.3、16.4、16.5、`Implementation.md` 2.1、3.1、14.3、`ADR-0006`
- 状態：設計契約。**Canister ID、署名鍵ID、実額上限は未確定**であり、この文書で値を捏造しない

## 1. 目的と原則

1. 環境は「network」「鍵」「endpoint」「eligibility issuer」の4点で分離する。1つの設定ミスでmockが本番へ通る状態を作らない。
2. 秘密（PEM、seed、identity、`.icp/data`）をリポジトリへ入れない。`.gitignore` で除外する。
3. 未確定値は「未確定」と記録し、判明した時点でこの表へ追記する。推測値を設定ファイルへ書かない。
4. 本番の不可逆な操作（controller除去、SNSローンチ、実資金受付）はPhase 0の対象外である。

## 2. 環境マトリクス

| 項目 | local | testnet | mainnet |
|---|---|---|---|
| IC network | `icp network start` のローカル（PocketICベース） | IC testnet（`ic` とは別の検証用） | IC mainnet |
| Canister ID | `icp deploy` が動的割当。`.icp/data` に保持し**コミットしない** | **未確定**（Phase 1で払い出し後に記録） | **未確定**（本番前に記録） |
| tECDSA key ID | local のテスト鍵（既定 `test_key_1`） | `fuqsr` 上のテスト鍵。key ID名は**未確認**（`test_key_1` を第一候補。`set_ecdsa_key_id` で上書き） | `key_1`（subnet `pzp6e`、34ノード）。Phase 2では拒否 |
| 署名subnet | local replica | `fuqsr` | `pzp6e`（fiduciary signing subnet） |
| HL REST | mock HL（ローカル） | `https://api.hyperliquid-testnet.xyz` | `https://api.hyperliquid.xyz` |
| HL WS（ブラウザ直結） | mock または未接続 | `wss://api.hyperliquid-testnet.xyz/ws` | `wss://api.hyperliquid.xyz/ws` |
| 資産 | 合成 | test USDC（HyperCore） | USDC（HyperCore） |
| builder fee | 0 | 0 | **未確定**（本番の事業判断。暗黙に徴収しない） |
| eligibility issuer | mock issuer（合成属性） | mock issuer（合成属性） | **未確定**（契約・法務確認後に登録） |
| 開発者controller | 許容 | 許容 | 本番目標では除去。除去はPhase 4の別途承認 |
| `APP_STAGE`（frontend） | `demo` | `demo` | 未設定（`demo`以外は503） |
| 実資金 | 扱わない | test USDCのみ | **Phase 0では扱わない** |

- `key_1`／`pzp6e` は `Implementation.md` 2.1の記載であり、本契約で再確認はしていない。Phase 1で実測して確定する。
- asset indexは`meta.universe`から解決し、固定値を埋め込まない。2026-09-19のtestnet実測ではBTC=3・ETH=4（mainnetはBTC=0・ETH=1）であり、network間で添字が異なる（`docs/phase-1/README.md` 6節）。
- ローカルの統合試験はPocketICサーバ（`.pocket-ic/`、16.0.0）で行う。ローカルネットワーク（`icp network start`）とは別のハーネスである。
- ローカルの閾値ECDSAは、PocketICの**テスト用閾値鍵サブネット**で有効になる。`PocketIc::new()`（既定トポロジ）には鍵が無いため `existing keys: []` で拒否される。`PocketIcBuilder::new().with_application_subnet().with_test_threshold_keys_subnet().build()` を使う（`crates/pocket-ic-tests/src/lib.rs`）。
- ローカルのkey idは **`test_key_1`**（2026-09-19に実測）。本番は `key_1`（`pzp6e`）。PocketIC上の署名往復は約17.9msだが、これはtestnet・本番subnetの性能値ではない。
- mainnetの署名鍵・subnetは本番リリース候補のビルドで再確認する（Phase 4）。
- Builder feeの上限同意・徴収アドレスは暗黙に決めない。`api-contract.md` のAgent承認とは別のmaster署名（`approveBuilderFee`）を要求する。

## 3. 鍵とderivation path

`Plan.md` 16.1に基づく規則。

- tECDSA鍵はCanister ID・derivation path・key IDに束縛される。同じCanisterの悪意ある新コードも署名できる前提で扱う。
- `funds_vault` が共通保管口座とユーザー別取引口座のmaster鍵を管理する。ユーザー別口座は独立masterの口座であり、共通masterのHL sub-accountではない。
- `trading_core` は口座別・世代別Agent鍵のみを管理する。
- derivation pathには暗号学的乱数の`account_id`（32バイト）と`generation`のみを使う。**EOA・Principal・`user_id`を公開pathやcloidへ埋め込まない。**
- 設計上のpath名（Phase 1で最終確定）:

| 用途 | path案 |
|---|---|
| 共通保管口座master | `["private-perp", "vault", "reserve"]` |
| ユーザー別取引口座master | `["private-perp", "trading", account_id_hex]` |
| Agent世代鍵 | `["private-perp", "agent", account_id_hex, generation]` |

- 導出結果は世代ごとにキャッシュする。失効・期限切れした鍵を再承認しない。Agent再生成は保存した`account_id`と`generation`に基づく（公開`user_id`を使わない）。
- 開発環境の開発者controllerは許容するが、本番相当の安全性を主張しない。

## 4. mock と本番の分離（必須の試験）

`Plan.md` 16.5、`ADR-0006` に基づく。Phase 1で実施する。

| # | 分離試験 | 合格条件 | 状態 |
|---|---|---|---|
| E-1 | mock eligibility tokenを本番相当の network・鍵・build設定で提示 | 拒否される | **未実施**（eligibility発行はPhase 3。tokenが存在しない） |
| E-2 | testnet用の設定で mainnet endpoint を指定 | 起動・受付が拒否されるか、明示的に失敗する | **実装・検証済み**（`hl-types`のホスト試験＋`core_environment.rs`・`vault_environment.rs`） |
| E-3 | HPKE要求の`aad`に別network・別canister・別method・別caller・別`request_id`・期限超過を混ぜる | すべて拒否される |
| E-4 | 別環境で発行したセッション・challengeを再利用 | 拒否される（`NetworkMismatch`／`OriginMismatch`） |
| E-5 | mock HL用のendpoint設定が本番ビルドへ混入 | ビルド時に検出され、そのままでは起動しない |
| E-6 | `APP_STAGE`が`demo`以外で公開ページ以外の応答 | 503かつ本人データを返さない |

- 環境判定はビルド時の設定ではなく、起動時に検証可能な値（network、canister、key ID、endpoint）で行う。
- mock token・mock issuer・mock HLの設定は「開発用」と明示し、本番設定のテンプレートへコピーしない。

### 4.1 起動時の環境設定（実装）

- 値はビルド定数ではなく、canisterの設定（`funds_vault.vault_config`／`trading_core.core_config`）に持つ。設定は controller のみが変更できる（`funds_vault` は `set_network`・`set_venue_endpoints`・`set_ecdsa_key_id`、`trading_core` は `set_market_context`（network・dex）・`set_venue_endpoints`・`set_ecdsa_key_id`）。`get_environment` は公開の診断queryで、秘密を含まない。
- 未設定は network 既定（`local`）とし、endpoint は network から解決する。**mainnet は Phase 2 では拒否**する（`mainnet_not_enabled`）。endpoint は network と整合する host のみ受け付ける（testnet → `api.hyperliquid-testnet.xyz`、local → ループバック等。**local の設定で実venueのhostを指定すると拒否**する）。
- tECDSA key ID は network 既定（local・testnetは `test_key_1`）とし、testnet の実名が確定したら `set_ecdsa_key_id` で上書きする。mainnet の `key_1` はPhase 2では到達しない。
- 検証は純粋クレート `hl-types::environment` に集約し、ホスト試験で固定する（mainnet拒否・host不一致・lookalike domain・key ID形式）。
- E-5（mock endpointの本番混入）は、mock endpoint を**コードへ埋め込まない**（設定でのみ与える）ことで構造的に満たす。ローカルの既定はループバックであり、実venueへは出ない。

## 5. 設定の出所

| 設定 | 出所 | コミット |
|---|---|---|
| Canisterビルド・配信 | `icp.yaml`（Phase 0で追加。environmentsはPhase 1で追加） | する |
| Canister ID | `icp` CLIの管理データ（`.icp/`）、`icp canister status <name> -i` | しない（`.icp/`は`.gitignore`） |
| identity・PEM | `icp identity` | しない |
| network・root key | `icp network status --json` | しない |
| 環境別の固定値（endpoint等） | `icp.yaml` の environments、またはcanister environment variables | する（秘密を含めない） |
| 起動時の環境設定（network・endpoint・key ID） | canisterの設定（controller専用のsetter。`get_environment`で確認） | しない（値はデプロイ時に設定） |
| 本番の実額上限・料金 | 事業判断の確定後 | する（確定後） |

- ローカルのroot keyは明示的に選択したローカル環境でのみ使用する（`icp-cli` の原則）。
- Canister間の連携先IDは、icp-cliが注入する環境変数（`PUBLIC_CANISTER_ID:<name>`）で渡し、ハードコードしない。

## 6. 未確定事項

| 項目 | 記録先 | 確定時期 |
|---|---|---|
| testnet/mainnet Canister ID | 本節の表 | Phase 1（払い出し後）／本番前 |
| test用key ID名とsubnet | 本節の表 | Phase 1 |
| HLのtestnet制限（最小額・手数料・確定イベント） | `money-and-units.md` 9節 | Phase 1 |
| 本番の総預かり上限・ユーザー上限・取引上限 | 本節の表 | Phase 4〜5 |
| 本番eligibility外部発行者 | 本節の表 | 契約・法務確認後 |
| `icp.yaml` の environments 記法の確定 | `icp.yaml` | Phase 1（`icp project show` で検証） |
