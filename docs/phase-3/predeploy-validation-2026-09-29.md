# デプロイ前検証（2026-09-29）

対象は `feat/single-canister-testnet` の `cd795ce5ff041550d24e9acecd03410940f42cdb` 上の作業中変更。公開IC・Cloudflareへのデプロイはしていない。

## 完了した検査

| 対象 | 結果 |
| --- | --- |
| Rust host workspace（wasm専用crateとPocketICを除く） | 81件成功、ignoreなし |
| PocketIC全試験 | 初回144件成功・4件失敗。失敗した市場監視1件、負荷2件、vault upgrade1件を修正後に各対象全件再実行して成功。計148件の成功を確認、ignoreなし |
| 本番用の統合WasmによるPocketIC | single_canister 1件、market_consensus 4件成功 |
| frontend unit | 47件成功 |
| ブラウザE2E（ローカル実canister + mock HL） | 4件成功、skipなし。認証・入金・保管・配分・Agent・注文・SL/TP・取消・決済・回収・出金・応答喪失・ログアウト・110件超の履歴・別口座分離を確認 |
| 実HL testnet読み取りsmoke | 1件成功。ICはローカル。testnet認証、口座準備、eligibility登録、実info APIの残高読み取り。exchange POSTなし |
| Cloudflare公開前検査 | 指定canister IDでtestnet build、生成されたdist/server/wrangler.jsonに対するwrangler deploy --dry-run成功。APP_STAGE=testnet、IC_HOST=https://icp-api.io、gzip 421.32 KiB。アップロードなし |
| Nodeテスト | PocketIC runner 7件、mock HL 8件成功 |
| Rust静的検査 | fmt、host全target Clippy、単体/統合wasm全target Clippy、no-await、signing-boundary成功 |
| frontend静的検査・build | lint、format、TypeScript、production build成功 |
| Candid/公開API | 統合112エンドポイント一致。6種類の本番Wasmから抽出したCandidを確認し、vaultのコメント・並び順のみ同期。テスト専用APIの混入なし |
| UI表示 | vlmkit 0.22.0: tradeの3 viewport integrity CLEAN、scroll/handlers/interactions/breakpoints成功（警告あり） |

PocketICは16を使用。production、単体mock、統合mockのWasmを別targetにビルドした。全件試験は `RUST_TEST_THREADS=1 cargo test --locked -p pocket-ic-tests --no-fail-fast`。再試験は `--test market_monitor`、`--test mixed_load`、`--test vault_upgrade`。本番用追加試験は `--test single_canister --test market_consensus`（`PRIVATE_PERP_UNIFIED_MOCK`なし）。CIのRust/frontend検査項目も実行した。

本番用統合WasmのSHA-256: `24bed4e14713cfd3b466e74c43d340abcbab4409f7e2070ae23d49184b79c74c`。統合mock版: `c8f6c6ca018f6fb0074094e1ad2f87b9e254c3f1ac540c8bcaac1f3fb235d6bd`。

## 試験で修正した点

- ローカル入金ボタンがmock入金後に照合を呼ばず待っていた。本人操作から `confirmDeposit` を一度呼ぶよう修正。照合失敗時に入金・照合・配分を自動再試行しない単体試験を追加した。
- 暗号化応答用の手書きCandidデコーダーから `JournalWriterBusy` が漏れていたため追加。復号後のデコード例外ではなく、通常の `CanisterError` として扱えることを単体試験で確認した。生成API型に対する網羅性をTypeScriptで検査し、今後の種別追加漏れも検出する。
- PocketIC runner試験のビルド回数・target期待値を統合Wasm分まで更新。
- 市場監視は銘柄別の判定コンテキストでmetaとbookを各1回取得するため4リクエスト・44weight。fixtureにbookのcoin/timeを追加した。
- 混合負荷試験は20/100ユーザーの配分・注文・取消・回収を確認。上限1200・退出予約300は変えず、使用量300超では次のworkflow前に61秒進める。待機後は市場を明示更新する。これはレート制限内での順次workflow試験であり、100人同時受付の性能保証ではない。
- vault upgrade試験は自動照合を期待せず、upgrade後も停止していること、本人の明示再開で履歴照合のみ行い不明送金を再POSTしないことを確認。
- ブラウザE2Eの操作手順を、初回注文前・画面復帰時・通信復旧時の明示的な取引情報更新に合わせた。SL/TPは2注文のOpenを確認してから次へ進み、出金は受付だけでなくSettledを待つ。資金履歴は依頼一覧なので110件の実配分依頼を作る。fixtureだけは同一依頼IDでJournalWriterBusyを処理し、ページングと口座分離を検証する。製品の自動再試行は追加していない。

## 留意点

- 失われたレバレッジcallbackのsnapshot復元試験だけは、復元直後upgradeのPocketIC install rate limitを無効化している。その他の本番特性や性能の保証には使わない。
- vlmkitの未認証ボタンの無効状態、チャート等の未実行イベントには警告が残る。ログイン後画面全体の視覚的合格を意味しない。スキル指定0.23.0は前回確認時に未公開で、導入済み0.22.0を使った。
- buildには大きいbundleとHPKE依存のNode crypto外部化に関する警告がある。ブラウザ実フローで暗号化通信の動作を別途検証する。
- 公開canisterのcycles、実HLへの署名送信・資金往復、公開UIの接続はこのローカル試験の合格では保証しない。

## 再現・後片付け

- ローカルE2E: `PATH=/private/tmp/private-perp-test-tools/bin:$PATH bash scripts/local-e2e.sh`。一時領域へcandid-extractor 0.1.6を用意し、プロジェクト専用ICP_HOME内の非匿名テストidentityを利用した。最終実行は4件成功（3.2分）。全体の制限時間を600秒にし、個々の操作・状態の検証タイムアウトは維持している。
- testnet読み取り: 同じ隔離されたICP_HOMEで新規ローカルnetworkを起動・統合canisterをinstallし、対象のlocal IDをTESTNET_APP_IDに指定して `scripts/prepare-local-testnet.sh` を実行した。専用テスト鍵のみ使用。公開canisterは操作していない。
- UI: VITE_APP_STAGE=testnet、IC_HOST=https://icp-api.io、PRIVATE_PERP_CANISTER_ID=xis3j-paaaa-aaaai-axumq-cai、MARKET_WS_URL=wss://api.hyperliquid-testnet.xyz/wsをVITE_*環境変数として指定し、`pnpm --dir frontend build --mode testnet`。続けて `pnpm --dir frontend exec wrangler deploy --dry-run --config dist/server/wrangler.json --outdir /private/tmp/private-perp-cf-dryrun`。
- テスト作成前には存在しなかったfrontend/.env.localと一時seedを削除した。スクリプトのcleanupでローカルnetwork・mockを停止。build成果物とignoredなローカルidentityは再利用可能な状態で残す。

実行ログは `/private/tmp/private-perp-{host-tests,full-pocketic,production-pocketic,load-retest,upgrade-retest,local-e2e,testnet-smoke,testnet-build,cf-dryrun}.log` に保存。全件PocketICログは初回の4失敗も残しており、修正後の成功ログと併せて読む。今回の成功確認は変更済み作業ツリーに対するもので、検証時点ではコミット・push・公開デプロイはしていない。
