# ローカル品質ゲート（2026-09-23）

対象は `617e8f9` に本変更を加えたローカル実装。以下の検証はすべて終了コード0で完了した。リモートCIや本番環境の合格を意味しない。

## 修正範囲

- 初回のPocketIC全件実行では7ファイル・18件が実口座ID不一致で失敗。正常系fixtureを実IDと明示的な口座観測へ移行した。汎用RPCヘルパでのID補正や本番の認可・鮮度条件の緩和は行っていない。
- その先にあった旧fixtureも修正した。取消応答のinner success、複数Candid引数、永続化された署名nonce、SL/TP照合時の建玉を現行契約に揃えた。誤口座・未観測・古い観測・許可外銘柄の拒否理由も検証する。
- runnerはロック取得失敗で停止し、自分が取得したロックだけを解放する。サーバー準備・build・testの失敗を伝播し、成功表示はtest完了後に限定する。本番とtest-venueの出力先を分離し、同一ディレクトリ指定を拒否する。CIも同じrunnerと回帰テストを使う。
- Canister本番コード、Candid、DB schema、frontendの動作は変更していない。

## 検証結果

- Rust: `cargo fmt --all --check`、CIと同じhost/wasm clippy（`-D warnings`）、hostテスト76件、`check-no-await.sh`、`check-signing-boundary.sh`が成功。
- PocketIC: `bash scripts/pocket-ic-test.sh --no-fail-fast`。最終版で29ファイル・85件が2回連続成功、失敗0・ignored 0。各回でfeatureなし本番Wasmとtest-venue Wasmを別々にビルドした。
- runner: `node --test scripts/pocket-ic-runner.test.mjs`、7件成功。正常終了、build失敗、test失敗、サーバー準備失敗、ロック競合、空のサーバーパス、出力先重複を検証した。
- 本番Wasm: 4 Canisterの成果物にテスト専用メソッドが含まれないことを検査して成功。
- frontend: `pnpm lint`、`pnpm format:check`、`pnpm typecheck`、`pnpm test`（28件）、`pnpm build`が成功。新規runnerテストも既存Oxfmt設定で整形・検査した。
- mock HL: `node --test tools/mock-hl/server.test.mjs`、7件成功。
- 実接続E2E: `bash scripts/local-e2e.sh`、Playwright Chromiumで4件成功、skipなし。ローカルICの `http://localhost:18100/` とmock HLを使用。終了後にnetwork停止と18100・8080・4173番ポートのlistener消滅を確認した。

未検証: リモートCI、実MetaMask、実HL、testnet・mainnet、実資金。frontend buildの既存bundle-size・crypto外部化警告は残る。コミット・push・公開は実施していない。
