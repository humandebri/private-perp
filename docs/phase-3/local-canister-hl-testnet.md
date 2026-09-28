# ローカルCanister + HL testnet

確認日: 2026-09-26。以下の確認結果は当時の旧5 Canister構成の履歴であり、現行の単一 `private_perp` Wasmの検証結果ではない。ICネットワークはローカル、外部venueだけが実HL testnet。公開ICPへのデプロイと実cycles費用の測定は行っていない。

## 今回の確認結果

- ローカルの5 Canisterを本番featureなしでデプロイし、vault/coreのnetworkを`testnet`、HL接続先を`https://api.hyperliquid-testnet.xyz`へ設定した。
- 実`meta`からBTC index 3・szDecimals 5、ETH index 4・szDecimals 4を取得した。模擬HLのindexを流用していない。
- 実`metaAndAssetCtxs`の出来高には小数6桁超があり、既存パーサーで市場観測が拒否された。市場出来高だけを整数演算で切り捨て、入出金・価格・数量の厳密な精度検査は維持した。修正後、Canister経由のBTC/ETH市場観測と新規リスク許可を確認した。
- テスト専用のランダムEOA鍵でchallenge認証、HPKE経由の取引口座・準備口座作成、合成eligibility署名の登録、testnet入金案内を確認した。HLで準備口座を読み取った残高は0。注文・資金移動の`/exchange` POSTは未実施。
- 準備口座の入金先は`0xf9b2b86555bde4bd83ce5b5590ae56d0fd9d1a0f`。これは今回のローカル状態から導出したHL **testnet専用**アドレス。現在値はgit管理外の`.icp-home/hl-testnet/public.json`で確認する。ネットワーク状態をリセットすると同じ口座を操作できる保証がないため、資金往復中は状態を保持する。
- `recovery_history_verified`はfalseのまま。実HLの履歴完全性はまだ未確認。
- 結合smoke 1件、実HL型の出来高を含むPocketIC市場監視1件、frontend unit 33件・型検査・lint、Rust整形検査が成功した。trading-coreのnative unit試験はSQLiteのWasm専用ビルド制約で実行できず、パーサーのCanister経路はPocketICで検証した。

## 再実行

プロジェクトの空の専用ローカルネットワークを起動し、管理者Principalを指定して単一Wasmをデプロイした後、設置したIDを明示する:

```sh
export TESTNET_APP_ID="$(icp canister status private_perp --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')"
bash scripts/prepare-local-testnet.sh
```

スクリプトはこのリポジトリの`.icp-home`を使い、mock issuerとテストEOAの秘密鍵をその中へ0600で保存する。鍵を表示・git登録・frontendへの埋め込みはしない。既知のE2E秘密鍵は使わない。ICの接続はloopbackだけで、HL接続先にはtestnetのみを指定する。実testnetの口座準備を実行するため、通常のunit試験からは隔離した。

この試験は専用CLI/HPKEクライアントで実行する。模擬入金seedは使わない。`bootstrap-local.sh`の通常動作はmockのままで、`HL_NETWORK=testnet`を明示した場合だけ実HLに接続する。testnetを設定した状態へmock bootstrapを実行して環境を混ぜないこと。現行の単一Canister版スクリプトの再実行結果はまだない。

## 残件

テストUSDCの入金後、Canisterの履歴照合で本人残高への計上を確認する。その後、少額の配分、Agent承認、注文、取消または決済、回収、出金を実行してHL側の履歴・残高と突き合わせる。実HLでの署名受理、送金受理、結果不明時の回収、2口座分離は現段階では合格に含めない。
