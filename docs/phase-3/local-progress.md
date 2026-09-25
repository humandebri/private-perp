# Phase 3 ローカル実装の進捗

更新: 2026-09-25。**Phase 3全体は未完了。testnetの受入試験はローカル実装完了まで保留。**
Phase 2の既存実装・判定とは区別する。

## BTC・ETH取引体験の判定

- ローカルの実canister・mock HL・Chromeで本人の入金案内、配分、Agent承認、成行・指値、SL/TP、取消、回収、出金、履歴、別ユーザー分離の一往復を確認した。画面は注文・建玉・資金を普段の導線とし、資格登録や運用詳細は必要時に開く。狭幅の退出操作もE2Eで確認した。
- 実HLでの操作感、注文受理と約定の待ち時間、残高反映の滑らかさは未判定。testnet受入はユーザーの指示に従い、ローカル実装が終わるまで行わない。
- 本人認可、単発送信、結果不明時の予約とフェンス、取消・決済・回収・出金の経路は取引体験の安全条件として維持する。古いbackupの全面再構築、builder fee mock、細かな100人性能内訳、相関評価はそれぞれ別判定として記録する。

## 実装済み

- policyの共有REST予算をguardから設定できるようにし、bootstrapでvault/coreをworker登録する。両workerのHL REST送信経路はpolicyの許可を送信前に取得する。許可の期限切れやpolicy障害では送信を始めない。
- 注文・配分・Agent承認は新規リスク、取消・出金・回収は退出、照合は照合の枠として計上する。HLの`userFills`は最大2000件を想定してweight 120を確保する。建玉・注文状態の問い合わせはweight 2を確保する。1200/分と退出予約300はローカル試験値であり、実測後に調整する。
- coreのsweepは取消、照合、注文の順で処理する。未解決注文のある口座を別の永続カーソルで優先し、通常の巡回カーソルも維持する。`userFills`の取得間隔は未解決注文ありで2分、その他は10分とし、最終取得時刻をupgrade後も保持する。
- reduce-only注文は緊急停止・新規銘柄allowlistの対象外にし、不要な`updateLeverage`送信を省く。POST結果不明の処理を再送せず、既存のunknown照合経路に残す。
- vaultのoutboxは予算許可を得てから送信状態へ遷移する。許可取得失敗でactionが署名済み状態に取り残されることを防ぎ、リース満了後に再試行できる。拒否・不明時の監査理由、入金照合失敗、注文再試行理由をコード化し、監査イベントに取引所の生メッセージや口座アドレスを保存しない。
- guardの権限、予算拒否後の再試行・単発送信をPocketICで追加検証した。Candidとfrontendのvault bindingを更新した。
- 回収受付は要求・取引残高予約・nonce・actionを一つのDBトランザクションで作る。coreの口座フェンスはvaultだけが操作し、prepareでローカル注文とHLのopenOrders・建玉を確認し、commit後に送信状態を確定してPOSTする。取消とreduce-only決済はフェンス中も利用できる。
- HLが`usdSend`の受理・拒否を明示したら、その応答で回収仕訳・予約・actionを確定し、coreのフェンス解除を永続キューから再試行する。POST応答を失った場合は再送せず、予約とフェンスを保持する。
- 送金の履歴は`startTime`・`endTime`でページ取得し、送金元・宛先・額・時刻・hashを照合する。500件の上限に達した窓は縮小して再取得し、照会失敗・曖昧な一致はunknownを維持する。未実行判定はnonceの約2日間と全ページ確認の後、testnetの履歴完全性確認を明示設定した場合に限る。networkまたはHL endpointを変更すると確認設定は自動で失効する。
- HL履歴の`usdc`をJSON数値・文字列の双方から整数演算でマイクロUSDCへ変換する。入金巡回とcontroller照合、回収のunknown照合で共通化し、丸めが必要な額・範囲外・不正な型は根拠として扱わない。変換関数は元のJSON型を保持する。
- coreのupgradeは新規リスクを全体停止し、vaultが旧未解決回収を世代付きフェンスへ移してから解除する。フェンス解除失敗は永続actionから再試行する。本人の`FundStatus`と画面には回収準備中・照合待ちを表示する。
- 独立`send_journal` canisterを追加し、vault/coreのHL POST前に要求ID・口座ID・nonce・digestをhash連鎖へ追記する。送信側は受領連番とhashを`dispatching`と同一DB transactionで保存し、毎送信前に独立ジャーナルの高水位を照合する。ジャーナル不通・連番不一致・未解決の旧actionに受領記録がない場合は新規POSTを止める。記録の範囲取得は登録worker本人に限定し、1回100件までとした。ローカルbootstrapとCandidに追加した。
- HL履歴からの入金は、tx hash・network・宛先・金額・観測時刻を非公開V2業務イベントへ先行追記してから、受領番号と台帳仕訳を同じDBトランザクションで確定する。重複したtx hashは再計上・再追記しない。入金以外の履歴型を正の金額だけで計上せず、管理下の回収送金も除外する。controllerによる入金直接注入は`test-venue`専用とし、本番Candidから削除した。
- 未知宛先入金の本人振替は、対象イベントID・本人ID・金額を非公開V2業務イベントへ先行追記し、受領番号とsuspenseから本人残高への仕訳を同じDBトランザクションで確定する。追記前後に未請求状態を確認し、重複請求とjournal不通時の残高変更を拒否する。
- 独立ジャーナルは入金・本人振替イベントの論理IDを内容から再計算して検証する。任意の論理IDで同じ入金や振替を別記録として追記することを拒否する。
- 初回ログインで新規EOAと本人IDの対応を作る際は、Principal・EOA・本人ID・networkを非公開V2業務イベントへ先行追記する。書込みフェンス内で既存本人を再確認し、受領番号と本人行を同じDBトランザクションで確定する。既存本人の再ログインは追記せず、journal不通中でも認証できる。journal不通中の新規本人登録は拒否する。
- vaultの本人向け書込み6入口（セッション失効、Agent承認、配分、出金、準備口座作成、回収）を単一のHPKE `private_call`へ移し、旧平文Candid入口を削除した。封筒の宛先・network・caller・期限・単回使用IDを照合し、業務結果も暗号化する。frontendの操作とローカルbootstrapの鍵生成を合わせた。
- ジャーナル照合ロックはupgrade後に自動解除せず、設定済みSNS principalだけが`control_guard.resume_journal`を通して解除できるようにした。この経路は独立ジャーナルの高水位・hashとローカル受領記録の連続性、受領記録のない未解決POSTの不存在を確認する。不一致のある古いbackupは送信停止のまま残す。
- coreの注文・Agent世代要求・全取消・個別/全決済を書込みHPKE `private_call`へ移し、旧平文Candid入口を削除した。vaultには配分前の取引口座準備をHPKEで追加し、画面が入金案内前に呼び出す。本人・他人のセッションを使ったPocketIC試験と旧入口拒否試験を追加した。
- 復元時に独立ジャーナルの差分があれば、guard経由の再開呼出しで最大100件ずつhash鎖を検証し、受領記録と別の永続ステージへ取り込む。ステージ済みでも送信は解除しない。`journal_restore_status`はcontrollerへローカル受領末尾・ステージ末尾・送信停止状態を返す。送信中に再開処理がロックを立てた場合、受領証跡を`dispatching`へ反映するトランザクションを拒否する。
- `send_journal`に送信意図とは別の、workerごとの版付き業務イベント列を追加した。本人と口座の対応、仕訳、予約、注文リスク、約定、外部結果、基準点の型を定義し、論理IDの冪等性、内容相違の拒否、連番と前hashを実装した。登録worker以外には取得させない。配分・出金・回収POSTの明示受理と拒否、回収の応答喪失後の履歴一致・未実行確定、coreのleverage設定・注文・取消の明示結果、配分・出金・回収と注文の受付、後続約定と注文状態観測は型付きイベントを先に追記し、受領番号を対応する状態確定と同じDB transactionで保存する。残る業務更新の先行追記は未実装。
- 業務イベント列に差分がある場合、guard経由の再開呼出しは1回最大100件を連番・前hash・worker hashで検証してvault/coreの永続ステージへ取り込む。controller専用の`recovery_stage_status`でローカル受領列またはステージの末尾連番と送信停止状態を見られる。本人・口座、注文状態・約定、準備口座への入金・未解決入金の本人振替は条件付きで差分反映する。外部照合のない反映は送信再開を許可しない。
- V2業務イベントがローカルの永続ステージに残っている場合、独立ジャーナルの高水位が0に巻き戻ってもguard再開と新規POSTを拒否する。V1送信意図はworkerごとの永続書込み世代フェンスで追記を直列化し、受領時に所有世代・次連番・hashを確認する。古いcallbackや追記応答不明で受領記録を作ってPOSTすることはできない。
- V2業務イベントも同じ永続書込み世代フェンスで直列化し、受領番号・payload・hashをローカル業務更新と同一transactionで保存する。V1送信意図またはV2イベントの追記応答が不明なときは要求IDまたは論理IDで独立ジャーナルを照会し、同一内容を確認できた場合だけ受領処理へ進む。確認できなければ送信を止める。V1/V2双方の独立高水位とローカル受領列が一致しない場合も送信を拒否する。
- coreの建玉観測はHLの十進文字列を整数のマイクロUSDCへ厳密に変換し、不正な`assetPositions`・建玉・損益・使用証拠金や6桁を超える精度を拒否する。失敗した観測では既存の建玉と観測時刻を保持する。
- eligibilityは市場policyと独立した規約版、issuerアドレス、署名済みtoken、使用済みnonceを永続化した。HPKEで取引口座準備・署名対象取得・token登録を行い、Principal、user ID、account ID、network、vault、規約版、発行時刻、期限、nonceに束縛したsecp256k1署名を検証する。local/testnetのmock issuer設定をguard経由で受け付け、mainnetでは拒否する。入金案内、配分、core新規注文の受付と送信直前で検査し、資格切れでも退出操作は通す。issuer秘密鍵は環境変数を読むローカルCLIのみで使用する。
- vault/coreにcycles残量・消費サンプル、設定日次下限、退出予約枠を永続化し、30日補充目標、7日通知、3日停止を計算する。下限未設定または予約枠到達でも新規受付を止める。coreは共有REST予算でBTC/ETHの`metaAndAssetCtxs`（20）と各`l2Book`（2）を5分間隔で取得し、期待index、上場状態、出来高、スプレッド、両側板厚を確認する。10分超の観測や取得失敗では新規注文を止める。画面に本人資格、cycles、市場の停止理由と観測時刻を追加した。
- builder feeの送信値を0に保ち、EOAの`personal_sign`で署名した同意をHPKEで登録するlocal/testnet専用mock経路を追加した。署名はPrincipal・ユーザー・口座・vault・network・builder・有効期限・nonce・fee 0に束縛し、mock承認と金額0の会計記録を同一DBトランザクションで保存する。実HLの`approveBuilderFee`は呼ばない。
- 固定seedの20人・100人で、各ユーザーの資格登録、合成入金、配分、Agent承認、注文、取消、回収を実際のcanister経路で混ぜた[ローカル負荷記録](mixed-load-local.md)を作成した。共有REST予算、待機、観測鮮度、canister別cycles総差分、失敗件数を記録した。V2資金送信結果イベントの通常追記後に再測定し、両群で失敗0件だった。

## 未実装・未検証

- 回収フェンスのtestnet受入。ローカルではcoreのprepare/commit、注文受付・送信前のフェンス、HL openOrders・建玉照会、vaultの予約とoutboxを結合した。POST結果不明は再送せず、送金履歴の時間窓と取得カーソルを永続化して照合する。実HLの履歴の完全性が未確認のため、未実行判定による自動解除は既定で無効にした。
- 2口座testnet受入のGATE 0を確認したが、この作業環境の`ICP_HOME=.icp-home`には`private-perp-local` identityのみで、`icp.yaml`にもlocal networkのみが定義されている。testnet canister ID、cycles、tECDSA key ID、2独立HL口座とtest USDCの準備を確認できていない。実HLへのPOST・残高照合・履歴完全性確認は未実施であり、`recovery_history_verified`をtestnetで有効にしていない。[受入記録](testnet-acceptance.md)にGATE 0と必要な証跡を分けた。
- V2先行追記の未網羅部分、状態digest付き基準点、注文本文を含まない復元記録からの注文行再構築、送信意図V1がbackupより進んだ場合の差分確定、HL履歴・残高による復元確定は未実装。本人・口座、入金、資金受付と一部外部結果、注文受付の要求とリスク予約は限定再生できるが、古いbackupを全面的に安全復元する条件には達していない。証跡不足のbackupでは送信停止を維持する。
- 署名・outcall・保存別のcycles実測は未実施。canister別の総差分しか得ておらず、[混合負荷](mixed-load-local.md)の100人試験もIC時計を進めて共有REST予算の待機を処理したため、実時間の性能合格には使えない。
- 2独立HL口座でのtestnet資金往復・障害復旧。testnet identity、cycles、test USDC、tECDSA key IDの準備状況も未確認。A/B0/B1の合成公開トレース相関評価は[別記録](privacy-local-eval.md)で実施し、B0/B1とも未達とした。実HLの公開情報を加えた相関評価は未実施。

## 検証

- PocketIC全件は本番Wasmとtest-venue Wasmを分離してビルドし、33ファイル・98件成功した。回収試験はcoreとの結合へ更新し、送信待ち・外部注文による未送信拒否、応答喪失、履歴照会失敗、500件ページ、約2日後の全ページ確認による未実行判定、upgrade時の移行ロックを確認した。
- 金額型修正後、PocketICでJSON数値・文字列の入金と回収、指数表記で正確に表せる額、精度超過・負値・範囲外の非計上、不正履歴と曖昧な複数候補での予約・フェンス維持を確認した。実HLのREST応答型と残高移動はまだ確認できていない。
- 金額型修正後のPocketIC全件は100件成功。`funds-vault`のWasm向けclippy（`-D warnings`）、Rust fmt、`git diff --check`も成功した。
- ジャーナル導入後、Wasm型チェックとPocketICの`vault_recovery` 10件、`core_pipeline` 5件、ジャーナルの認可・冪等・内容相違・範囲取得1件、coreだけ古いsnapshotへ戻す停止試験1件が成功した。これらは局所試験であり、この段階のPocketIC全件は未実施。
- 固定seedの合成A/B0/B1評価は生成・公開トレース攻撃・正解表採点を別プロセスにし、20人/100人の調整用・未見群を実行した。B1は未見群でtop-1 45%/49%、A比55%/51%削減となり、基準の20%以下・80%以上削減に未達。部分・全退出と損益ありの同額回収が相関の主因。これは性能・cycles・実HLの合格ではない。
- 同じ入力と未見群で退出の回収・払い出しを異額へ分割し時間分離する`B1Exit`比較腕を追加した。本人の回収済み残高以上を払い出さない条件を試験で確認した。公開額を時間窓で集計する攻撃を加えた結果、未見20人/100人のtop-1は45%/48%で、いずれも相関基準に未達。シミュレーション以外の配分・退出実装は変更していない。
- eligibility登録を含むPocketICのvault配分負荷は20人/100人で各20/100 POST、REST weight 20/100、受付p95ホスト時間26/22ms、失敗0件だった。vault/policy/journal別cycles総差分を[負荷記録](load-local.md)へ保存した。未再生のV2業務イベント後に追加配分POSTが始まらないことも確認した。
- [混合負荷](mixed-load-local.md)では20人と100人の配分・注文・取消・回収POSTを各20/100件確認し、REST weight累計6,840/63,264、失敗0件を記録した。ただし実時間の待機と署名・outcall・保存別cyclesは測れておらず、性能判定は未達。
- coreだけの古いsnapshot復元とjournal canister停止をPocketICで注入し、どちらもHL `/exchange` POSTが出ないことを確認した。Agent承認の明示拒否後に同世代を再承認する既存試験がjournal ID衝突で失敗したため、確定拒否後の再試行へ新しい要求IDを割り当て、unknown時は`approving`を保持して再試行させない形に修正した。
- 画面は回収フェンスとAgent期限で新規注文を止め、REST予算不足の待機文言と狭幅の全取消・全決済・回収・出金導線を追加した。frontendの型検査・lint・unit 29件は成功。狭幅Playwright試験はインストール済みChromeを使って成功した。実canister接続を要するE2E 1件は環境未設定でskipされた。
- vaultのHPKE入口移行後に`vault_agents`と`vault_funds`のPocketIC試験が成功し、frontend型検査・lint・unit 29件が成功した。旧平文メソッドを直接拒否する試験を追加した。
- HPKE入口とguard再開変更後のPocketIC全件は成功した。`send_journal` 2件、`vault_hpke` 2件、`core_pipeline` 7件も局所実行で成功した。guard経由の再開試験は無権限の直接呼び出しとguardへの無権限呼び出しを拒否する。
- coreの書込みHPKE化後、PocketICの`core_close` 4件、`core_hpke` 4件、`core_orders` 12件、取引口座準備後の`vault_multi_user` 1件と`vault_hpke` 2件が成功した。ジャーナル差分の永続ステージと送信停止の試験を加え、PocketIC全件も成功した。frontend型検査・lint・unit 29件、Rust fmt、`git diff --check`が成功した。
- Rust wasm clippy `-D warnings`、fmt、`check-no-await`、`check-signing-boundary`、Candid抽出とfrontend binding生成を実行した。
- 今回のPocketIC全件は37 test binary・113件成功した。その後のジャーナルV2高水位確認、issuer鍵ローテーション制約を含む対象試験（eligibility、send_journal、market_monitor、local_load）も成功した。frontendのlint・型検査・unit 29件・build、狭幅を含むPlaywright 3件が成功し、実canister接続E2E 1件はskipした。相関評価を再実行し、未見B1/B1Exitは20人でtop-1が各45%、100人で49%/48%となり基準未達を維持した。
- builder fee mockと混合負荷を追加した後のPocketIC全件は39 test binary・116件成功した。復元業務イベントの永続ステージを加えた後、`send_journal`結合5件を再実行して、V2イベントの取り込み・送信停止・controller限定状態表示を確認した。Wasm clippy `-D warnings`、frontend lint・format・型検査・unit 30件・build、Rust fmt、`git diff --check`も成功した。
- 2026-09-25、V2高水位の巻き戻り拒否、V1/V2の永続書込みフェンスと受領hash検査、core建玉金額の厳密変換を追加した。変更途中のPocketIC全件39 test binary・116件は成功した。資金送信のV2結果イベントを追加した後は`send_journal`・`vault_outbox`・`vault_recovery`の結合試験25件と20/100人の配分負荷・混合負荷各2件を再実行して成功した。新しいcyclesと待ち時間は[配分負荷](load-local.md)と[混合負荷](mixed-load-local.md)へ記録した。Wasm `cargo check`・clippy `-D warnings`、`check-no-await`、`check-signing-boundary`、Rust fmt、`git diff --check`も成功した。最新変更後のPocketIC全件再実行は未実施。
- V1送信意図に登録worker限定の要求ID検索を追加し、追記応答不明時の同一内容照会へ接続した。`send_journal` 5件は成功した。coreのleverage設定・注文・取消の結果イベント接続後、`core_close`・`core_orders`・`core_pipeline`・`core_triggers`の対象試験と注文結果の非公開証跡確認が成功した。最新の20人/100人混合負荷も失敗0件で再測定し、[記録](mixed-load-local.md)を更新した。Wasm clippy `-D warnings`、送信境界の静的検査、fmtとdiff検査も成功した。PocketIC全件は37 test binary・116件成功した。その後追加した、V1は一致するがV2だけ巻き戻ったジャーナルの停止試験を含む`vault_outbox` 11件も成功した。
- 復元ステージのV1末尾を最大連番だけで判断しないようにし、ローカル受領列からの連続性・最初の前hashを確認する。V1/V2両方に差分があるbackupは1回のguard再開でそれぞれ最大100件をステージしてから送信停止を返す。`send_journal`のPocketIC 5件で両列の同時取り込みと送信停止を確認した。ステージを受領済みとして扱う経路は追加していない。
- 配分・出金の受付、予約、reserve nonce、送信action作成を各要求の単一DBトランザクションへ統合した。action ID取得の`await`後に本人セッションを再検証し、配分はeligibilityとcycles、出金は署名intentの期限を再確認する。PocketICの`vault_funds` 6件、`vault_multi_user` 1件、`vault_outbox` 11件と`funds-vault`のWasm clippy `-D warnings`が成功した。これは業務イベントの先行追記・古いbackupの再構築を完成させる変更ではない。
- 新規のreserve/trading保管口座について、本人ID、口座ID、種別、導出経路、公開アドレス、networkをV2業務イベントに先行追記し、受領番号と口座行を同一DBトランザクションで確定する。署名秘密は記録しない。別呼出しが鍵導出中に先に作成した口座は再確認して返し、追記後のローカル確定が失敗した場合は送信を停止する。既存口座の復元には今後の状態digest付き基準点が必要で、まだ実装していない。PocketICの`send_journal` 5件、`vault_outbox` 11件、`vault_recovery` 10件で変更後の経路を確認した。
- 口座生成イベント追加後、固定seedの[配分負荷](load-local.md)と[混合負荷](mixed-load-local.md)を20人/100人で再測定した。全群でPOST件数が一致し、失敗0件だった。ジャーナルcyclesは口座生成の追記分だけ増えた。回収要求も乱数取得の`await`後にセッションを再確認するよう変更した。
- coreがvaultから取得した本人・取引口座・取引所アドレスの対応を初めて保存する際、登録worker限定の`IdentityAccount`業務イベントを先行追記し、受領番号と口座行を同一DBトランザクションで保存する。既存行の本人が異なる場合は拒否する。局所PocketICの`core_orders` 12件で注文結果イベントと口座対応イベントを確認した。これだけでは既存口座や古いbackupの全面再構築はできず、状態digest付き基準点と残りの業務イベントが必要。
- 古いcore backupに対してV1送信意図が一致し、差分V2が検証済みの口座対応イベントのみである場合、guard経由の復元処理で口座対応と受領番号を同一DBトランザクションへ取り込む。イベントの論理ID・連番・前hash・payload hashを再検証する。取り込み後は`replay_pending_validation`を永続化して送信停止を維持し、再度guardを呼んでも解除しない。controller専用の`recovery_replay_pending`で、この未検証状態をvault/coreそれぞれ確認できる。基準点と残りの業務状態を照合できるまで自動再開はしない。誤った論理IDを拒否するPocketICを含む`send_journal` 7件が成功した。
- 口座対応イベント追加後のPocketIC全件は39 test group・117件成功した。さらに口座IDへの別本人・別アドレス上書きを共通DB層で拒否し、変更後に`core_orders`・`core_pipeline`・`vault_recovery`の29件、ローカル実canister・mock HL・Chromeの画面E2E 4件を再実行して成功した。口座対応だけの限定取り込みとcontroller用状態照会を追加した後、PocketIC全件を再実行して成功した。Wasm clippy `-D warnings`、送信境界の静的検査、Rust fmt・diff検査、frontend lint・整形・型検査・unit 30件・buildも成功した。画面E2Eは限定取り込み追加後には再実行していない。
- 入金の先行V2追記後、PocketIC全件39 test group・119件が成功した。`vault_reconcile`では本人計上と重複抑止に加え、非入金イベントの除外、非公開イベントの記録、journal停止時の未計上を確認した。ローカル実canister・mock HL・Chromeの画面E2Eも4件成功し、入金案内から配分・注文・取消・回収・出金まで再確認した。Wasm clippy `-D warnings`、送信境界の静的検査、Rust fmt・diff検査、frontend lint・整形・型検査・buildも成功した。
- 未知宛先入金の振替イベント追加後、`send_journal` 7件と`vault_deposits` 2件が成功した。後者では非公開イベント、二重請求拒否、journal停止時の振替拒否と本人残高維持を確認した。Wasm clippy `-D warnings`とCandid抽出も成功した。この追加後のPocketIC全件と画面E2Eは未実施。
- 入金・振替の論理ID検証追加後、`send_journal` 7件、`vault_deposits` 2件、`vault_reconcile` 1件が成功した。誤ったIDの拒否と正常な入金・振替を確認した。Wasm clippy `-D warnings`、Rust fmt・diff検査も成功した。PocketIC全件と画面E2Eはこの追加後には未実施。
- 本人登録イベント追加後、PocketIC全件39 test group・119件、初回・再ログインとjournal停止時を確認する`vault_auth` 7件、ローカル実canister・mock HL・Chromeの画面E2E 4件が成功した。Candid抽出、Wasm clippy `-D warnings`、Rust fmt・diff検査も成功した。本人・口座以外の差分再構築と状態digest付き基準点は未実装のまま。
- 画面では普段の注文・建玉・資金導線を優先し、受付資格の登録と運用詳細を必要時に開く表示へ整理した。761〜900px幅の取引画面の横溢れも修正した。ローカル実canister・mock HL・ChromeのE2E 4件が成功し、本人の入金案内、配分、Agent承認、成行・指値、SL/TP、取消、回収、出金、履歴ページング、ログアウト後の別ユーザー分離を確認した。testnetでの操作感や実HLの滑らかさは未検証。
- 取引画面の通常表示を取引口座資産・出金可能額・建玉数、チャート、注文、建玉、注文一覧へ絞った。板と公開約定、注文の事前確認・送信状態は開閉式の詳細へ移し、建玉がない場合も空状態を表示する。市場観測・鮮度の正常時バッジを省き、異常時の理由と退出導線は表示する。固定の初期価格をなくし、本人の入力または接続中の市況からの参考価格取込を注文条件にした。接続前のモバイル画面と幅320〜1280pxの横溢れ検査を実施した。実口座での操作感は未検証。
- 古いvault backupのV2差分が連続した本人登録イベントである場合、guard経由で本人・EOA対応と受領番号を同一DBトランザクションへ限定的に取り込む。論理ID、network、前hash、worker hashを検証し、取り込み後も`replay_pending_validation`と送信停止を維持する。本人対応の回復とguard再実行による停止維持を含む`send_journal` 8件が成功した。基準点と業務状態・HL証跡の全面照合は未実装。
- coreの口座対応差分を再生するとき、同じ本人・HLアドレスの既存口座行は更新しない。差分イベントに口座の再有効化権限はないため、復元DBにある停止状態と所有権確認時刻を維持する。対応不一致は拒否し、新規口座の行だけを作る。復元の最終再開条件は従来どおり未達で、送信停止を維持する。
- vaultの限定復元は本人登録に続くreserve/trading保管口座のイベントも扱う。論理ID、前hash、network、導出経路、アドレスの所有者を検証し、既存口座は状態を維持して一致確認だけを行う。保管口座の論理IDは独立ジャーナルへの追記時にも検証する。異なるnetworkのイベントは取り込まず送信停止を維持する。PocketICの`send_journal` 9件で保管口座の復元、異なるnetworkの拒否、誤った論理IDの追記拒否を確認した。鍵導出の実照合、台帳・予約・HL外部証跡の全面復元は未実装。
- 保管口座イベントの追記検証変更後、通常の配分・outbox・回収を含むPocketIC 27件が成功した。Wasm workspace clippy `-D warnings`、Rust fmt、差分検査も成功した。実HLと古いbackupの全面復元はこの局所検証の範囲外。
- 配分受付では、本人・取引口座・金額・予約action ID・reserve nonceをV2業務イベントへ先行追記し、受領番号と要求・予約・actionを同一DBトランザクションで確定する。追記前に残高と重複要求、書込みフェンス内でnonceを再確認する。独立ジャーナル停止時は予約を作らない。PocketICの`vault_funds` 7件で重複要求がイベントを増やさないこととジャーナル停止時の残高維持を確認した。固定seedの20人・100人配分負荷と混合負荷も成功した。負荷試験は未反映V2イベントを注入した後の配分受付拒否、予約残高維持、POST停止を確認する期待値へ更新した。このイベントの差分再生、注文リスクの先行追記、状態digest付き基準点は未実装であり、古いbackupの送信停止は維持する。
- 出金受付は本人署名と出金intentのnonceを検証し、準備口座を確定してから、本人・口座・宛先EOA・金額・両nonce・期限・action IDを非公開V2へ先行追記する。回収受付も取引口座・準備口座・金額・口座別nonce・action IDを先行追記する。どちらも受領番号と要求・予約・actionを同一DBトランザクションで確定し、追記前に重複・残高・nonceを確認する。PocketICでは重複受付がイベントを増やさず、journal停止時に出金・回収の拘束を作らないことを確認した。受入イベントの差分再生とHL証跡照合は未実装。
- 出金・回収の受付イベント追加後、PocketIC全件39試験群・124件が成功した。固定seedの20人・100人配分負荷と混合負荷、回収フェンス、出金intentの単回使用、journal停止時の予約維持を含む。ローカル実canister・mock HL・Chromeの画面E2Eも4件成功し、資金往復・注文・取消・履歴ページングを再確認した。Candid抽出、Wasm全workspaceのclippy `-D warnings`、送信境界の静的検査、Rust fmt・差分検査も成功した。実HLと古いbackupの全面復元は引き続き未判定。
- coreの注文受付は、要求・注文ID・本人と口座・cloid・本文digest・予約元本・reduce-onlyを登録worker専用V2イベントへ先行追記する。受領番号と要求・リスク予約・pending注文を同一DBトランザクションで確定し、書込みフェンス内と確定直前に重複、口座フェンス、未終端注文上限、equity上限を再検査する。journal不通では新規注文とリスク予約を作らず、同一要求の再送もイベントを増やさない。署名済み取引所本文と注文の平文パラメータはV2へ含めないため、このイベントだけでは古いbackupのpending注文を再構築できない。送信停止を維持し、復元形式は今後の課題とする。
- 注文受付イベント追加後のPocketIC全件は39試験群・124件成功した。受領時の重複記録拒否とjournal停止時の新規リスク予約拒否を明示したcoreの注文・パイプライン・決済23件は、内部リファクタリング後にも再実行して成功した。Candid抽出、Wasm全workspaceのclippy `-D warnings`、送信境界の静的検査、Rust fmt・差分検査も成功した。ローカル実canister・mock HL・Chromeの画面E2E 4件も成功し、注文から回収・出金まで再確認した。testnet受入と古いbackupの全面再構築は未実施。
- 配分受付変更後のPocketICは全38試験群・122件を確認した。負荷試験と巻き戻し試験の旧い「ジャーナル不整合後も受付は成功する」という期待値を、受付拒否・予約残高維持・POST停止へ更新したため、最初の全件実行は該当箇所で停止した。修正後にそこまでの33試験群・98件と、outbox以降の5試験群・24件を成功させた。Wasm全workspaceのclippy `-D warnings`、Candid抽出、Rust fmt、送信境界の静的検査、差分検査も成功した。
- ローカル実canister・mock HL・Chromeの画面E2Eも4件成功し、入金案内、配分、Agent承認、注文、取消、回収、出金、履歴ページング、ログアウトを再確認した。履歴ページング用の大量配分fixtureは、単一書込みフェンスによる一時的な`PolicyUnavailable`に対し、同一業務要求IDを維持して有界に再試行する。frontend lint・format・型検査も成功した。これは同時配分の性能合格を意味せず、通常の20人・100人負荷とは別に扱う。
- ローカルbootstrapがpolicy版をguardを通さず直接設定していたため、新規注文が`policy is not configured`で拒否されていた。SNS認可済みguardの設定経路を追加し、bootstrapで設定結果を読み戻して失敗を検出するようにした。guardのPocketIC試験では直接設定と無権限設定を拒否し、SNS経由の設定を確認した。E2Eでは別Principalで履歴ページング用の配分を作る際にも、当該Principalに束縛した受付資格をHPKEで登録した。local cycles下限と退出枠はローカル専用の値へ調整し、testnetへの流用はしない。frontend lint・format・型検査・unit 30件、control-guardのWasm clippy、Rust fmt、diff検査も成功した。
- 取引画面の整理と固定初期価格の廃止後、ローカル実canister・mock HL・ChromeのE2E 4件が成功した。参考価格の明示取込から注文・約定・取消、回収・出金、履歴、ログアウトと再ログインを確認した。frontend lint・format・型検査・unit 30件・buildも成功した。`vlmkit` 0.22.0による取引画面のintegrityはdefect 0、幅320〜1280pxのbreakpoint sweep・横溢れ・操作検査は成功した。0.23.0はnpm未公開のため公開済み版を使用。市況mock停止時のWebSocket接続警告は残る。Wasm clippy `-D warnings`、送信境界の静的検査、Rust fmt・diff検査も成功した。
- BTC・ETH画面の主要残高をreserve・取引口座・出金可能額に絞り、証拠金・損益・観測鮮度は資金詳細へ移した。新規受付停止の理由は一つの開閉可能な通知にまとめた。建玉と注文一覧は狭幅でカード状にし、個別取消・部分/全決済を横スクロールなしで操作できるようにした。ローカル実canister・mock HL・Chromeの一往復E2Eは4件成功し、390px幅の個別全決済表示と横溢れ0を確認した。`vlmkit` 0.22.0の取引・資金画面integrityはdefect 0、取引画面の幅320〜1280px境界と横スクロール検査も成功した。これは実HLでの操作感の合格ではない。
- coreの`userFills`は本人ID・取引口座ID・HL oid・銘柄を揃えて注文を特定する。約定のtid・oid・本人/口座/注文ID・銘柄・数量・価格・fee・時刻を登録worker専用のV2業務イベントへ先行追記し、受領番号と約定・リスク更新を同じDBトランザクションで確定する。ジャーナル停止時は約定を反映せず、重複tidや注文と違う銘柄は追記しない。注文ごとの約定量は浮動小数点を使わず厳密に合算し、注文量を超える履歴は拒否する。旧`Fill`型を保持して新`FillObserved`型を追加し、保存済みイベントの読み取り互換性を守る。PocketIC全39試験群・124件、ローカル実canister・mock HL・Chromeの一往復E2E 4件が成功した。型の分離後も約定とジャーナルのPocketIC局所試験、Wasm clippy `-D warnings`、Candid抽出、送信境界の静的検査が成功した。約定イベントの古いbackupへの再生と、残る業務更新の先行追記・基準点・外部証跡照合は未実装。
- coreの`orderStatus`照合は、本人の口座とHL oidで注文を一意に特定し、`open`・`filled`・`cancelled`・`rejected`への変化を注文ID・口座ID・外部応答digest付きV2イベントへ先行追記する。受領番号と注文・リスク更新は同じDBトランザクションで確定する。journal停止時は取消結果を反映せず、未知のHL状態と終端後の古い観測は無視する。取消後に遅れて届いた約定は記録するが、注文状態を再び未終端へ戻さない。Candid抽出、Wasm clippy `-D warnings`、送信境界の静的検査、PocketICの注文状態2件とジャーナル検証を再実行した。`unknown`から`open`への実応答を伴う試験、古いbackupへのイベント再生、外部証跡による復元確定は未実施。
- 分割約定2件が注文数量へ到達する場合と、取消後に2件の遅延約定が届く場合をPocketICで確認した。DBは注文単位の約定数量を十進整数で合算し、注文量を超える取り込みを拒否する。journalへの追記前に数量・価格の不正値を拒否する。対象PocketIC 2件、Wasm clippy `-D warnings`、Rust fmt、差分検査が成功した。全PocketICとローカル画面E2Eはこの変更後には未再実行。
- coreの古いsnapshotへ戻した後、V2の注文状態と約定をhash鎖・論理ID・本人/口座/注文の対応を確認して差分反映する経路を追加した。取消状態と遅延約定2件の復元をPocketICで確認した。ステージの反映後も`replay_pending_validation`を保持し、外部HL履歴・残高の照合がない限りSNS guard経由の`resume_journal`でも送信を再開しない。controller専用のguard principal照会をCandidとfrontend bindingへ追加した。対象PocketIC、Wasm clippy `-D warnings`、frontend lint・format・型検査は成功。binding生成CLIの`didc`がこの環境にないため、該当メソッドを既存の同型Resultへ追加して型検査した。状態digest付き基準点と、注文受付・台帳・予約・外部結果の差分再生は未実装。
- vaultの未解決入金の本人振替は、受付時とジャーナル差分再生時に登録済みuser ID、元のsuspense仕訳、入金額、未請求状態を確認する。古いvault snapshotへ戻したPocketICで本人振替仕訳を再生し、`replay_pending_validation`と送信停止を維持した。controller専用guard照会をvaultのCandidとfrontend bindingにも追加した。対象PocketIC、ジャーナル結合9件、Wasm clippy `-D warnings`、frontend lint・format・型検査が成功した。資金予約・送金結果の差分再生と外部照合による再開は未実装。
- 入金の仕訳をvaultと復元系が共用するDB処理へ移した。古いvault snapshotから、口座ID・宛先・network・状態が一致する準備口座への`DepositCredit`を再生し、3件の入金で本人残高が復元されることをPocketICで確認した。口座対応のない入金はsuspenseへ誤分類し得るため、取引口座への入金は配分の移動中残高との対応が必要なため、どちらも再生せずステージと送信停止を維持する。未解決入金が止まるPocketIC試験も成功した。vault入金試験2件、Wasm clippy `-D warnings`、Rust fmt・差分検査が成功。状態digest付き基準点、資金予約・送金結果の差分再生とHL外部照合は未実装。
- vaultの`FundTransferResult`は、古いsnapshotからの差分再生時にV1送信意図の受領、actionの送信状態・nonce・本人・口座・要求、資金予約と宛先を照合してから、配分または出金の仕訳・予約・要求状態を同一トランザクションで反映する。独立ジャーナル側でも回収・配分/出金・注文結果イベントの論理IDを内容から再計算し、異なる論理IDによる重複追記を拒否する。PocketICでは配分と出金のPOST応答直前のvault snapshotへ戻し、結果イベントから残高と予約を復元できること、外部照合が済むまで送信停止を維持することを確認した。対象の出金・配分試験、send-journal 10件、最新のPocketIC全面試験はすべて成功。Wasm clippy `-D warnings`、Rust fmt、差分検査も成功した。回収結果の差分再生、全受付イベントの再構築、状態digest付き基準点、HL履歴・残高による復元確定は引き続き未実装。
- 回収の明示POST応答を`RecoveryPostResult`として履歴判定の`RecoverySettlement`から分離した。送信意図のV1受領、送信中action、本人・取引口座・準備口座・予約・nonce・宛先の一致を確認した場合だけ、明示応答の仕訳と予約を古いsnapshotへ再生する。履歴判定イベントと旧形式の直接応答イベントは証跡の種類を区別できないため、差分ステージに保持して送信停止を続ける。受理・拒否の応答直前snapshotからの復元に加え、応答喪失後の履歴確定前snapshotへ戻しても履歴イベントだけではunknownと予約・フェンスを解除しないことをPocketICで確認した。回収11件とsend-journal 10件が成功し、Candid抽出とWasm clippy `-D warnings`も成功。履歴証跡の再構築、受付イベントの再生、状態digest付き基準点、外部照合による再開は未実装。
- vaultの配分・出金・回収受付イベントを、古いsnapshotからのV2差分再生対象に追加した。本人と口座の対応、要求ID・本文fingerprint・nonce・送金先・残高予約を検証し、受付要求、予約、必要な仕訳、送信待ちaction、受領番号を同一DB transactionで再構築する。`usdSend`の本文とdigestは署名処理と再生処理で共通の実装を使う。各受付直前snapshotへ戻すPocketIC試験で予約・本人署名nonce使用済み状態・回収フェンスを確認し、外部照合が済むまで送信停止が続くことを確認した。変更後のPocketIC全39群・129件、Wasm clippy `-D warnings`、Rust fmt、差分検査が成功した。復元の全件照合とSNS経由の安全な送信再開、状態digest付き基準点、未再生の業務イベントは引き続き未実装。
- V1送信意図が古いbackupより進んでいても、独立ジャーナルのV1差分が末尾までステージされ、その連番とhashが遠隔高水位に一致した場合は、V2受付イベントの先頭部分だけを再生する。V1ステージは送信済み受領として扱わず、POST結果の確定や送信再開も行わない。配分受付前snapshotへ戻した後、元の環境ではPOST成功済みのケースで、予約が戻り、結果は未確定のまま、再POSTが出ないPocketIC試験を追加した。send-journal 10件、vault-outbox 15件とWasm clippy `-D warnings`が成功した。V1送信可能性の照合・受領への昇格と外部証跡による復元確定は未実装。
- coreの`OrderAccepted`差分から、本人・口座の対応、要求の論理ID、cloidとorder IDの導出、リスク額を検証し、受付要求と新規リスク予約を同一DB transactionで復元する。reduce-onlyは予約を作らない。署名済み本文も平文注文本文もジャーナルにないため送信可能な注文行は作らず、外部照合まで停止を維持する。受付前snapshotへ戻すPocketICで125 USDC相当の予約が戻り、注文行は戻らないことを確認した。ジャーナル側もcloidとorder IDの不一致を拒否する。core-orders 13件、send-journal 10件、Wasm clippy `-D warnings`と差分検査が成功した。注文行とHL外部状態を照合した復元完了は未実装。
- vault/coreの公開読取APIにローカルのジャーナル送信停止・復元差分の照合待ち状態を追加し、frontendの新規注文判定と停止理由の表示へ接続した。送信停止時は口座残高と履歴を閲覧できるが、送信再開までは退出操作が成功すると案内しない。APIは再開権限を持たず、遠隔ジャーナル障害は次の送信前照合まで検出されない。Candidとfrontend bindingを更新し、PocketICの復元試験で停止状態を確認した。狭幅の全取消・全決済にも通常表示と同じ確認を追加した。
- 直近の全PocketICは並列実行によるPocketIC instance削除競合を避けて逐次実行し、39試験群・131件が成功した。この全面実行後の公開ジャーナル状態APIは対象のcore-orders 13件・vault-outbox 15件とWasm clippyで確認した。ローカルの実Canister・mock HL・Chromeの一往復E2E 4件も成功した。狭幅の確認ダイアログ追加後のE2E再実行では4件中3件が成功し、長い一往復試験は資金履歴の110件入力後、別口座へのログインが`PolicyUnavailable`で失敗した。確認ダイアログの取消は通過したが、最後の別口座ログインの失敗原因は未確定であり、この再実行を合格とは記録しない。
- `vlmkit` 0.22.0でビルド済みローカル画面を検査した。取引・資金・履歴の公開状態は1280/768/375pxのintegrityで欠陥0、取引画面の320〜1280px幅掃引と375pxでの横はみ出し0、タッチ対象不足0、測定可能な文字コントラスト違反0、開閉操作・animation・reduced-motion検査も通過した。フォーカス検査の高さ720pxで出た逆行警告は、1600pxで消えたためスクロール中のviewport座標に起因する。残る段飛び警告は非操作要素と無効ボタンを飛ばす箇所であり、Tab到達性の欠落は確認されていない。未接続のボタン3件を操作不能とする`check interactions`の警告は意図した無効状態。`funds`と`history`は未認証シェルのみを測ったため、本人画面の視覚合格を意味しない。スキルの要求版0.23.0はnpm未公開で、公開済み0.22.0を使用した。
- ローカル検証は実HLのREST weight・受理挙動・cycles費用を保証しない。

## 既存環境の移行

新しい`RecoveryPostResult`を理解できるよう、`send_journal`をvaultより先にupgradeする。旧`RecoverySettlement`は直接応答と履歴判定を区別できないため、復元差分に現れた場合は送信停止を維持する。coreを先にupgradeすると新規リスクが全体停止する。次にvaultをupgradeし、core principalを初回設定してvaultのsweepを実行する。vaultは旧未解決回収を世代付きフェンスへ移し、全件が移った場合だけcoreの移行ロックを解除する。`recovery_migration_locked`と本人の`FundStatus.recovery_fence`で状態を確認する。移行中も取消・reduce-only決済は続けられる。testnetではHL履歴の完全性を確認するまで`recovery_history_verified`を有効にしない。

## ローカルeligibility issuer

`LOCAL_ELIGIBILITY_ISSUER_KEY`をgit管理外の環境変数に設定し、`cargo run -p eligibility-issuer -- address`で公開アドレスを得る。これを`LOCAL_ELIGIBILITY_ISSUER_ADDRESS`として`bootstrap-local.sh`へ渡す。画面で取得した署名対象のCandid hexをissuer CLIの標準入力へ渡し、出力された署名を画面へ登録する。testnetでは公開アドレス・規約版・cyclesと市場閾値を別途設定し、ローカルbootstrapの値をそのまま流用しない。
