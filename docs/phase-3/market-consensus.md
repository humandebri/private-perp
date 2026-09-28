# 市場の受付判定に対する合意

HL の `metaAndAssetCtxs` と `l2Book` は取得中にも変動する。応答全体の JSON 正規化だけでは replicated HTTPS outcall の合意が成立しなかったため、各 IC ノードの transform で受付条件を評価し、その判定を合意対象にする。

## 信頼境界

通常の replicated HTTPS outcall を維持する。単一ノードの応答やブラウザーからの数値で注文を許可しない。閾値と対象銘柄、要求開始時刻は Canister が transform context に固定する。transform は永続状態を書き換えず、各ノードの生の応答を検査して次の判定だけを返す。

- metadata: 対象銘柄の一意性、配列の対応、上場廃止フラグの型、登録 index、最低出来高。
- order book: 対象銘柄、要求開始から前後60秒以内の時刻、両側1〜20段、正の価格・数量、板の整列、非交差、最大スプレッド、最低板厚。
- 欠損・不正なデータは `Invalid`。HTTP status は保持し、200以外は呼び出し側が拒否する。

スプレッドは整数への切捨て前の比率で比較する。出来高は小数部を切り捨て、過大評価しない。異なる数値でも受付条件が同じなら同じ判定となる。閾値を跨ぐ変動によって IC が判定に合意できない場合は取得失敗となり、新規注文を停止する。

metadata は元 API に観測時刻がないため、その時刻の検証は行えない。HTTP 応答取得を含む一回の観測全体を60秒以内に制限し、板については元応答の時刻も検査する。市場の観測頻度5分・失効10分は従来どおり。これは口座 snapshot の10秒鮮度条件とは別の、市場流動性による受付条件である。

## 永続化と競合

BTC/ETH の応答を取得した後、閾値が取得開始時と一致し、その観測がまだ処理中であることを確認して一括保存する。設定変更や後続観測がある場合、古い成功・失敗応答はそれらを上書きしない。合意していない価格・出来高・板厚の具体値は保存せず、既存の任意診断列は NULL とする。保存 schema と公開 Candid は変更しない。

cycles の既存ルールで新規リスクを停止している間、自動の市場取得も停止する。取消・照合は継続し、controller による明示的な市場更新は診断用に残す。これによりすべての自動 outcall の費用がなくなるわけではない。

## 検証

`scripts/test-single-canister.sh` は本番・試験用 Wasm の両方で、変動する安全な応答が同じ判定になること、閾値超過・不正データ・別銘柄・古い板・HTTPエラーの拒否、取得中の閾値変更、再設定後の復帰を PocketIC で検証する。既存の認証・journal・upgrade と模擬送金・注文の統合検証も継続する。PocketIC の成功だけでは公開 subnet の合意成立を証明しないため、公開環境での確認は [受入記録](testnet-acceptance.md) に記載する。

参照: [ICP HTTPS outcall の安全性](https://docs.internetcomputer.org/guides/security/https-outcalls/)、[HL info endpoint](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint)。
