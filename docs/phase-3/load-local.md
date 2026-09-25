# Phase 3 固定seedのローカル負荷試験

実行日: 2026-09-25。`POCKET_IC_BIN=.pocket-ic/pocket-ic`、`POCKET_IC_WASM_DIR=target/test-venue/wasm32-unknown-unknown/release`で`cargo test --locked -p pocket-ic-tests --test local_load -- --nocapture --test-threads=1`を実行。seedは`20260924`、同一vaultに20人/100人のセッション、準備口座、署名済みeligibility tokenを作り、合成入金、配分受付、署名、V1送信意図とV2口座生成・結果イベントの記録、mock HLへのPOSTを通した。送信はPocketICの同期処理であり、実ネットワークの同時負荷ではない。

| 利用者 | HL POST | REST weight | 受付p95（ホストms） | vault cycles | policy cycles | journal cycles | 失敗 |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 20 | 20 | 20 | 22 | 210,918,030,970 | 164,860,297 | 1,713,237,846 | 0 |
| 100 | 100 | 100 | 27 | 1,055,189,790,363 | 836,744,055 | 8,580,576,068 | 0 |

配分POSTを1件につき1 weightとして計上し、V1送信意図の高水位とPOST件数を確認した。V2は口座生成イベントも含むため、固定件数ではなく測定時の高水位から未再生イベントを1件追加したことを確認する。次の配分POSTが始まらないことも確認した。cyclesは口座準備・認証・受付・署名・outcall・保存を含むcanister別の総差分で、署名/HTTPS outcall/保存ごとの内訳には分けられていない。実testnetのcycles費用、REST待ち時間、口座観測の鮮度、注文・回収を混ぜた100人の同時実行、障害注入時の失敗率は未測定である。したがって負荷試験全体の合格判定は**未達**。相関評価の未達とは独立に記録する。
