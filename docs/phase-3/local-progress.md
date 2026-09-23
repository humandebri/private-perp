# Phase 3 ローカル実装の進捗

更新: 2026-09-23。**計画全体は未完了。現時点は共有REST予算の基盤のみ。**
Phase 2の既存変更は保持している。testnet・mainnet・実資金への利用は対象外。

## 実装した範囲

- policy DB migration v3に登録worker・設定・消費履歴を追加。MemoryIdとv1/v2は変更しない。
- controllerはvault/coreのworkerを各1件だけ登録できる。登録先の差替え・同一principalの兼任は拒否する。
- guardが予算を設定し、登録workerだけが消費する。送信期限の60秒後まで保持した計上額の合計と新規リスク用上限を単一トランザクションで検査する。返金APIはない。
- `capacity=1200`、`exit_reserve=300`を試験値とする。実HLの制限を保証しない。未設定では消費を許可せず、設定上限10,000により有効な消費履歴の件数も制限する。
- 32-byte要求IDの先頭8 bytesは期限のbig-endian表現。期限は取得・送信開始の共通期限で最大60秒。同じIDの再取得と期限だけの変更を拒否する。期限の60秒後を過ぎた履歴だけを1回につき最大100件削除する。
- 緊急停止は新規リスクのみ拒否する。予算の復旧停止はoperatorが設定し、SNSのみ解除できる。復旧停止中は照合だけを許可する。
- 公開statusは設定・集計値・停止状態のみ。個別のcaller・要求IDは公開しない。Candidを同期した。

## 未接続の境界

この予算APIはvault/coreのREST送信経路へ**まだ接続していない**ため、現在の注文・資金処理をレート制限するものではない。guardからの設定実行経路とbootstrapも未接続。

接続時のworker契約: `consume_rest_budget`の`Ok`を確認した要求についてのみ1回送信できる。送信開始直前に`request.valid_at(now)`と業務状態を再検証し、awaitを挟まず送信を開始する。遅れて届いた応答・期限切れの許可を使わない。`valid_at`単体は予算取得の証明ではない。計上は`expires_at + 60秒`まで残すため、期限ぎりぎりに送信しても直後に枠は戻らない。実際の送信が早い場合や未送信の場合も保守的に保持する。これはworker送信開始の予算であり、ネットワーク遅延後のHL到着時刻まで保証するものではない。

`pause_for_recovery` / `clear_recovery_pause`は予算の停止状態だけを変更する。口座フェンス、復元操作、照合完了証明、送信済み要求の高水位記録は実装しておらず、このAPIだけで安全なrestoreができるとは扱わない。

## 残作業

1. 送信前の予算取得、取得後の状態再検証、未知結果の非再送、共有予算の設定経路と公平な巡回。
2. vault所有の口座フェンス・coreとのprepare/commit・建玉と未解決注文がゼロの場合だけの回収。
3. 書込みHPKE拡張、ローカルeligibility、Agent更新、cyclesと退出予備枠。
4. 非rollbackジャーナル・管理されたrestore手順・照合を条件にした再開・監査と保持期限。
5. mock Builder fee同意と会計、銘柄構成変更、UI・複数ユーザーE2E・20/100ユーザー負荷・固定seed相関評価。

## 検証範囲

`policy_budget`は登録者制限、共有上限と退出枠、同時要求、停止権限、同一コードへのupgrade保持、再設定で消費が消えないこと、個別期限、失効IDの再利用拒否、不正設定を検証する。

レビュー修正前の基盤で確認した結果:

- Rust fmt、host/wasm clippy（`-D warnings`）、host試験は成功。
- PocketIC全30ファイル・91件が2回連続成功。新規の`policy_budget`は6件。各回で本番Wasmとtest-venue Wasmを分離してビルドした。
- runner回帰7件、DB内await禁止・署名境界検査、Candid抽出・`didc check`、差分の空白検査は成功。
- UI変更なし。frontend・mock・ブラウザE2Eは今回再実行していない。コミット・デプロイも行っていない。

旧v2 Wasmからのmigration、REST実送信との結合、restore後の再送防止、Phase 3全体の受入条件は未検証。既存の全体品質ゲートは[Phase 2検証記録](../phase-2/local-quality-gate.md)と区別する。

### レビュー指摘への対応（2026-09-23）

- 取得時刻ではなく送信期限の60秒後まで集計・保持するよう修正。既存の`expires_at`と取得時刻indexを使用し、ic-sqlite-vfs 2.0.0（sqlite-precompiled）、MemoryId 120、schema v3は維持した。
- 59秒後の送信を想定し、61秒後の二重枠取得を拒否する回帰を追加。途中のGCでも計上を保持し、期限の60秒後を過ぎてから枠が戻ることを確認した。REST実送信を行う試験ではない。
- worker登録済み・予算未設定で`PolicyUnavailable`を検証し、設定後に同じ要求が成功することも確認した。
- 対象PocketIC 9件（`policy_budget` 7件、`policy_stop` 2件）、api-types/db host試験、policy/db wasm clippy、PocketIC testsのhost clippy、fmt、Candid抽出・構文検査、await・署名境界・差分検査が成功。全件・UI/E2Eは今回再実行していない。
