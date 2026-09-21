# Phase 2：単一ユーザーtestnet MVP（計画と進行）

`Implementation-Roadmap.md` §6 の実装計画。Phase 1の細部の作り込みより、**testnetで資金往復を通すこと**を優先する。

## GATE 0：前提（未達なら testnet へ進まない）

| # | 前提 | 状態 | 必要な操作 |
|---|---|---|---|
| G0-1 | IC testnet用identityとcycles、4 canisterのデプロイ承認 | **未** | `icp identity`でidentity作成 → faucetでcycles取得 → `icp deploy`（`funds_vault`/`trading_core`/`control_guard`/`policy_registry`） |
| G0-2 | HL testnet口座＋test USDC、MetaMask | **未** | HL testnet faucetでUSDC受領、MetaMaskにtestnetを追加 |
| G0-3 | testnet tECDSA key IDの確定（`test_key_1`第一候補・未確認） | **未** | デプロイ後 `ecdsa_public_key` を実測し `docs/phase-0/environments.md` へ記録 |
| G0-4 | HL testnetの制限（最小額・手数料・確定イベント） | **未** | 預入・出金を1往復して実測 |

## マイルストーン

| M | 内容 | ゲート |
|---|---|---|
| M1 | GATE 0 | 4 canisterがtestnetで起動し、key id確定 |
| M2 | 2A 環境設定の一般化／2B HPKE封筒の個人API適用 | PocketIC＋E-1/E-2 |
| M3 | 2C 建玉・PnL・SL/TP照合／2D 取消・Cancel All・決済／2E 鮮度ゲート | PocketIC、ローカルで代替フロー |
| M4 | 3A ウォレット認証／3B IC接続＋封筒クライアント／資金フロー | testnetで預入→配分→回収→出金 |
| M5 | 3C 取引画面／3D 非正常状態 | 受け入れシナリオ3〜5 |
| M6 | 3E 最小代替クライアント／Playwright／計測レポート／終了レビュー | §6の完了条件 |

## Phase 2で扱わないもの
Phase 3以降（複数ユーザー分離・負荷・backup復元・cycles通知・eligibility/監査/保持削除）、実資金・mainnet（E-2で**拒否**を試験）、Phase 1の細部（UIの磨き込み等）。ただし `reconcile_all` の固定窓（入金先が3件以上で古い口座が対象外）は実バグのためM3までに修正する。
