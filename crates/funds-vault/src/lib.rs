//! `funds_vault` Canister。
//!
//! 責務はEOA認証、共通保管口座とユーザー別取引口座のmaster鍵管理、複式台帳、
//! 資金要求・予約・outbox、Agent承認・失効である
//! （`docs/phase-0/authority-matrix.md` 2節、`docs/phase-0/api-contract.md` 2節）。
//!
//! **任意のダイジェストへの署名APIを追加しない。** 資金移動は目的・金額・宛先・
//! 本人認可・残高を検証してから行う。
//!
//! S2の2B時点で実装しているのは認証（challenge・セッション）である。資金API・outboxの
//! 署名送信・HPKEは後続の段階で追加する。

mod auth;
mod clock;
mod config;
mod crypto;
mod deposits;
mod environment;
mod fund;
mod outbox;
mod random;
mod venue;

/// HPKE封筒は共有クレートへ移設した（`trading_core`も同じ封筒を使う）。
use hpke_envelope as hpke;

use api_types::auth::{ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle};
use api_types::error::ErrorCode;
use candid::Principal;

const MEMORY_ID: u8 = db::memory_id::FUNDS_VAULT_MAIN;

thread_local! {
    /// 直近の入金照合時刻（timerの起動間隔より長い周期で回すためのゲート）。
    static LAST_DEPOSIT_RECONCILE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// このビルドのバージョン。デプロイ確認用。
#[ic_cdk::query]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// ログインchallengeを発行する。
#[ic_cdk::update]
async fn issue_challenge(request: ChallengeRequest) -> Result<ChallengeResponse, ErrorCode> {
    auth::issue_challenge(request, ic_cdk::api::canister_self()).await
}

/// challengeを消費してセッションを発行する。
#[ic_cdk::update]
async fn open_session(request: OpenSessionRequest) -> Result<SessionHandle, ErrorCode> {
    auth::open_session(request, ic_cdk::api::msg_caller()).await
}

/// セッションを失効させる。
#[ic_cdk::update]
fn revoke_session(session: SessionHandle) -> Result<(), ErrorCode> {
    auth::revoke_session(&session, ic_cdk::api::msg_caller())
}

/// HPKEの鍵世代を更新する（controllerのみ）。
///
/// 秘密鍵はcanister内のDBに留め、公開鍵のみを配布する（`Plan.md` 16.5）。
#[ic_cdk::update]
async fn rotate_hpke_key() -> Result<api_types::Blob, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can rotate the HPKE key".to_string(),
        });
    }
    let ikm = crate::random::random32().await?;
    let (secret, public) = hpke::derive_keypair(&ikm);
    let secret: [u8; 32] = secret.try_into().map_err(|_| ErrorCode::Internal {
        code: "unexpected secret length".to_string(),
    })?;
    let public: [u8; 32] = public.try_into().map_err(|_| ErrorCode::Internal {
        code: "unexpected public length".to_string(),
    })?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| db::repo::hpke::insert_key(connection, &secret, &public, now))
        .map_err(|error| auth::map_db(error, None))?;
    Ok(public.to_vec().into())
}

/// 取引所の入金（ledger update）を記録する（controllerのみ）。
///
/// 正規化したイベントID（`keccak256("deposit" ‖ tx_hash)`）で**二重計上を防ぐ**。
/// 本番ではreplicatedな`/info`照合がこの経路を呼ぶ。ユーザーへの紐付け（宛先アドレス→
/// 利用者）と`deposit_confirmed`の起票は次段階（アドレス写像の実装後）に行う。
#[ic_cdk::update]
fn ingest_venue_deposit(
    tx_hash: api_types::Blob,
    amount: u64,
    asset: String,
) -> Result<bool, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can ingest venue deposits".to_string(),
        });
    }
    if tx_hash.as_ref().is_empty() {
        return Err(ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "tx_hash must not be empty".to_string(),
        });
    }
    let mut input = b"deposit".to_vec();
    input.extend_from_slice(tx_hash.as_ref());
    let event_id = hl_sign::keccak256(&input);
    let now = clock::now_ms();
    let network = environment::network_name()?;
    db::tx::update(|connection| {
        let event = db::repo::events::ExternalEvent {
            event_id,
            network: network.clone(),
            account_address: [0u8; 20],
            counterparty: [0u8; 20],
            asset: asset.clone(),
            amount,
            kind: "deposit".to_string(),
            at: now,
            evidence_ref: Some(hex::encode(tx_hash.as_ref())),
        };
        db::repo::events::ingest_external_event(connection, &event, now)
    })
    .map_err(|error| auth::map_db(error, None))
}

/// テスト専用：queuedなactionのダイジェストを壊す（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[ic_cdk::update]
fn test_corrupt_action_digest() -> Result<u32, ErrorCode> {
    db::tx::update(|connection| db::repo::actions::overwrite_queued_digest(connection, &[0u8; 32]))
        .map(|changed| changed as u32)
        .map_err(|error| auth::map_db(error, None))
}

/// テスト専用：現行鍵で封筒を作る（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[ic_cdk::update]
async fn test_hpke_seal(plaintext: api_types::Blob) -> Result<api_types::Blob, ErrorCode> {
    let public = db::tx::query(db::repo::hpke::active_public)
        .map_err(|error| auth::map_db(error, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    let seed = crate::random::random32().await?;
    let aad = test_aad(0);
    hpke::seal(
        &public,
        b"private-perp/envelope/v1",
        &aad,
        plaintext.as_ref(),
        &seed,
    )
    .map(|envelope| envelope.into())
    .map_err(|error| ErrorCode::Internal { code: error })
}

/// テスト専用：封筒を開ける（`aad`の`expires_at`を変えると失敗する）。
#[cfg(feature = "test-venue")]
#[ic_cdk::query]
fn test_hpke_open(
    envelope: api_types::Blob,
    expires_at: u64,
) -> Result<api_types::Blob, ErrorCode> {
    let secret = db::tx::query(db::repo::hpke::active_secret)
        .map_err(|error| auth::map_db(error, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    let aad = test_aad(expires_at);
    hpke::open(
        &secret,
        b"private-perp/envelope/v1",
        &aad,
        envelope.as_ref(),
    )
    .map(|plaintext| plaintext.into())
    .map_err(|error| ErrorCode::BadRequest {
        code: api_types::error::BadRequestCode::MalformedPayload,
        detail: error,
    })
}

/// テスト用の`aad`（呼び出し元と期限を束縛する）。
#[cfg(feature = "test-venue")]
fn test_aad(expires_at: u64) -> Vec<u8> {
    let caller = ic_cdk::api::msg_caller();
    hpke::envelope_aad(
        "local",
        &ic_cdk::api::canister_self().as_slice().to_vec(),
        "test_hpke",
        caller.as_slice(),
        &[],
        expires_at,
    )
}

/// controllerのみ許可する（設定変更の共通チェック）。
fn require_controller(reason: &str) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if ic_cdk::api::is_controller(&caller) {
        Ok(())
    } else {
        Err(ErrorCode::Unauthenticated {
            reason: reason.to_string(),
        })
    }
}

/// 環境のnetworkを設定する（controllerのみ）。
///
/// mainnetはPhase 2では拒否する（`docs/phase-0/environments.md` E-2）。
#[ic_cdk::update]
fn set_network(network: String) -> Result<(), ErrorCode> {
    require_controller("only a controller can set the network")?;
    hl_types::environment::parse_network(&network).map_err(environment::map_environment)?;
    let now = clock::now_ms();
    db::tx::update(|connection| db::repo::vault_config::set_network(connection, &network, now))
        .map_err(|error| auth::map_db(error, None))
}

/// Hyperliquidのendpointを設定する（controllerのみ）。
///
/// 設定済みのnetworkと整合しないhost（例：testnet設定にmainnet endpoint）は拒否する。
#[ic_cdk::update]
fn set_venue_endpoints(exchange_url: String, info_url: String) -> Result<(), ErrorCode> {
    require_controller("only a controller can set the venue endpoints")?;
    let network: hl_types::Network = environment::network()?.into();
    hl_types::environment::validate_endpoints(network, &exchange_url, &info_url)
        .map_err(environment::map_environment)?;
    let now = clock::now_ms();
    db::tx::update(|connection| {
        db::repo::vault_config::set_venue_endpoints(connection, &exchange_url, &info_url, now)
    })
    .map_err(|error| auth::map_db(error, None))
}

/// 閾値ECDSAのkey IDを設定する（controllerのみ）。
///
/// testnetの鍵名はデプロイ後に実測して確定する（`docs/phase-0/environments.md` 2節）。
#[ic_cdk::update]
fn set_ecdsa_key_id(key_id: String) -> Result<(), ErrorCode> {
    require_controller("only a controller can set the ecdsa key id")?;
    hl_types::environment::validate_key_id(&key_id).map_err(environment::map_environment)?;
    let now = clock::now_ms();
    db::tx::update(|connection| db::repo::vault_config::set_ecdsa_key_id(connection, &key_id, now))
        .map_err(|error| auth::map_db(error, None))
}

/// 現在の環境設定（診断用・公開）。秘密は含まない。
#[ic_cdk::query]
fn get_environment() -> Result<api_types::environment::EnvironmentView, ErrorCode> {
    environment::resolved()
}

/// 現行のHPKE公開鍵。未生成はエラー（機密性の前提が欠けている）。
#[ic_cdk::query]
fn get_hpke_public_key() -> Result<api_types::Blob, ErrorCode> {
    let public =
        db::tx::query(db::repo::hpke::active_public).map_err(|error| auth::map_db(error, None))?;
    public
        .map(|public| public.into())
        .ok_or(ErrorCode::PolicyUnavailable)
}

/// 本人の取引口座アドレス（着金確認や照合に使う）。
#[ic_cdk::query]
fn get_trading_address(session: SessionHandle) -> Result<api_types::Blob, ErrorCode> {
    let status = auth::session_status(&session)?;
    let user_id: [u8; 32] =
        status
            .user_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::Internal {
                code: "user_id must be 32 bytes".to_string(),
            })?;
    let account = db::tx::query(|connection| {
        db::repo::ledger::custody_account(connection, &user_id, api_types::AccountKind::Trading)
    })
    .map_err(|error| auth::map_db(error, None))?;
    account
        .map(|account| account.master_address.to_vec().into())
        .ok_or(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        })
}

/// 本人の残高（`trading_core` がsnapshotを作るための参照）。
///
/// 戻り値は `(取引口座の残高, 出金可能額)`。認可のcaller束縛は呼び出し側（core）が
/// `session_status` で行う。
#[ic_cdk::query]
fn get_balances(session: SessionHandle) -> Result<(u64, u64), ErrorCode> {
    let status = auth::session_status(&session)?;
    let user_id: [u8; 32] =
        status
            .user_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::Internal {
                code: "user_id must be 32 bytes".to_string(),
            })?;
    let session_id: [u8; 32] =
        session
            .session_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::Internal {
                code: "session_id must be 32 bytes".to_string(),
            })?;
    let verified = auth::VerifiedSession {
        user_id,
        session_id,
    };
    // 取引口座のequityは **口座ID** で導出する（`user_trading:<account_id>`）。
    // 利用者IDで引くと常に0になる。出金可能額と同じ導出（`user_balances`）を使う。
    let status = fund::fund_status_with_holds(&verified)?;
    Ok((status.trading_equity, status.withdrawable))
}

/// 本人の取引口座ID（`trading_core` が所有権の確認に使う）。
#[ic_cdk::query]
fn get_trading_account(session: SessionHandle) -> Result<Option<api_types::Blob>, ErrorCode> {
    // canister間（trading_core）からの呼び出しを想定し、caller束縛は呼び出し側で行う
    // （coreは先に session_status で本人のprincipalを確認する）。
    let status = auth::session_status(&session)?;
    let user_id: [u8; 32] =
        status
            .user_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::Internal {
                code: "user_id must be 32 bytes".to_string(),
            })?;
    let account = db::tx::query(|connection| {
        db::repo::ledger::custody_account(connection, &user_id, api_types::AccountKind::Trading)
    })
    .map_err(|error| auth::map_db(error, None))?;
    Ok(account.map(|account| account.account_id.to_vec().into()))
}

/// 渡されたAgentアドレスを世代へ承認する（master鍵で署名して送信し、結果を永続化する）。
///
/// `generation` は `trading_core` が採番した世代を渡す。承認はvaultの
/// `agent_generations` に保存され、取引所の応答が不明な場合は `active` にしない。
#[ic_cdk::update]
async fn approve_agent_generation(
    session: SessionHandle,
    generation: u64,
    agent_address: api_types::Blob,
) -> Result<api_types::fund::AgentGeneration, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    let agent_address: [u8; 20] =
        agent_address
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::BadRequest {
                code: api_types::error::BadRequestCode::MalformedPayload,
                detail: "agent_address must be 20 bytes".to_string(),
            })?;
    fund::approve_agent_generation(&verified, generation, agent_address).await
}

/// 口座・世代の承認状態（`trading_core` が状態表示と署名可否の判断に使う）。
#[ic_cdk::query]
fn get_agent_approval(
    account_id: api_types::Blob,
    generation: u64,
) -> Result<Option<api_types::fund::AgentGeneration>, ErrorCode> {
    let account_id: [u8; 32] =
        account_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::BadRequest {
                code: api_types::error::BadRequestCode::MalformedPayload,
                detail: "account_id must be 32 bytes".to_string(),
            })?;
    db::tx::query(|connection| db::repo::agents::generation(connection, &account_id, generation))
        .map_err(|error| auth::map_db(error, None))
}

/// セッションの有効性（canister間の検証経路。呼び出し元は返却されたprincipalを検証する）。
#[ic_cdk::query]
fn session_status(session: SessionHandle) -> Result<api_types::auth::SessionStatus, ErrorCode> {
    auth::session_status(&session)
}

/// 入金案内（認証済みセッションが必要）。
#[ic_cdk::query]
fn get_funding_instructions(
    session: SessionHandle,
) -> Result<api_types::fund::FundingInstructions, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    fund::funding_instructions(&verified)
}

/// 資金状態（認証済みセッションが必要）。
#[ic_cdk::query]
fn get_fund_status(session: SessionHandle) -> Result<api_types::fund::FundStatus, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    fund::fund_status_with_holds(&verified)
}

/// 資金履歴（認証済みセッションが必要）。
#[ic_cdk::query]
fn list_fund_events(
    session: SessionHandle,
    cursor: Option<api_types::Blob>,
    limit: u32,
) -> Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    fund::fund_events(&verified, cursor, limit)
}

/// 配分を要求する（受付＋予約）。
#[ic_cdk::update]
async fn request_allocation(
    request: api_types::fund::AllocationRequest,
) -> Result<api_types::fund::FundRequestAccepted, ErrorCode> {
    let verified = auth::verify_session(&request.session, ic_cdk::api::msg_caller())?;
    fund::request_allocation(&verified, &request).await
}

/// 出金を要求する（本人署名の検証＋受付＋予約）。
#[ic_cdk::update]
async fn request_withdrawal(
    request: api_types::fund::WithdrawalRequest,
) -> Result<api_types::fund::FundRequestAccepted, ErrorCode> {
    let verified = auth::verify_session(&request.session, ic_cdk::api::msg_caller())?;
    fund::request_withdrawal(&verified, &request).await
}

/// テスト専用のECDSA往復（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[ic_cdk::update]
async fn test_ecdsa_roundtrip(
    seed: u32,
    digest: api_types::Blob,
) -> Result<(api_types::Blob, api_types::Blob), ErrorCode> {
    let path = crypto::derivation_path(&[b"test", &seed.to_be_bytes()]);
    let public_key = crypto::public_key(path.clone()).await?;
    let digest: [u8; 32] = digest
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "digest must be 32 bytes".to_string(),
        })?;
    let signature = crypto::sign_with_key(&digest, path, &public_key).await?;
    Ok((
        public_key.to_vec().into(),
        signature.to_bytes65().to_vec().into(),
    ))
}

/// テスト専用の入金計上（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[ic_cdk::update]
fn test_credit_deposit(
    session: SessionHandle,
    amount: u64,
    event_id: api_types::Blob,
) -> Result<(), ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    let event_id: [u8; 32] = event_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "event_id must be 32 bytes".to_string(),
        })?;
    fund::test_credit_deposit(&verified, amount, &event_id)
}

/// 定期sweepの起動を予約する（本番のみ・5秒間隔）。
///
/// **heartbeatではなくグローバルtimer**を使う（heartbeatはメッセージが無くても
/// 毎ラウンド呼ばれ、アイドル時もコストが乗る）。timerはアップグレードで失われる
/// ため`init`と`post_upgrade`の両方で予約する。正しさは永続状態（actionの
/// `queued`／`dispatching`と仕訳）が担保し、timerの継続には依存しない
/// （`Implementation.md` 6.3、`state-machines.md` 5節）。
#[cfg(not(feature = "test-venue"))]
fn schedule_sweep() {
    ic_cdk_timers::set_timer_interval(
        core::time::Duration::from_millis(outbox::SWEEP_INTERVAL_MS),
        || async {
            let now = clock::now_ms();
            // 1回の失敗でtimerを止めない（次の間隔で再試行する）。
            if let Err(error) = outbox::sweep(now).await {
                ic_cdk::println!("fund outbox sweep failed: {error:?}");
            }

            // 入金の定期照合（60秒間隔・1回あたり2件まで。outcallの回数を抑える）。
            let due_deposits = LAST_DEPOSIT_RECONCILE.with(|cell| {
                if now.saturating_sub(cell.get()) < 60_000 {
                    false
                } else {
                    cell.set(now);
                    true
                }
            });
            if due_deposits && let Err(error) = deposits::reconcile_all(2).await {
                ic_cdk::println!("deposit reconciliation failed: {error:?}");
            }
        },
    );
}

/// テスト専用のsweep（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[ic_cdk::update]
async fn test_sweep_now() -> Result<u32, ErrorCode> {
    outbox::sweep(clock::now_ms()).await
}

/// 呼び出し元のPrincipal（診断用。認可の判断は各メソッド内で行う）。
#[ic_cdk::query]
fn caller_principal() -> Principal {
    ic_cdk::api::msg_caller()
}

/// 取引所の入金を本人へ計上する（controllerのみ。宛先が導出口座の場合）。
#[ic_cdk::update]
fn credit_venue_deposit(
    tx_hash: api_types::Blob,
    amount: u64,
    address: api_types::Blob,
    asset: String,
) -> Result<bool, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can credit venue deposits".to_string(),
        });
    }
    if tx_hash.as_ref().is_empty() {
        return Err(ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "tx_hash must not be empty".to_string(),
        });
    }
    let address: [u8; 20] = address
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "address must be 20 bytes".to_string(),
        })?;
    let now = deposits::now_ms();
    let network = environment::network_name()?;
    db::tx::update(|connection| {
        deposits::credit(
            connection,
            &network,
            tx_hash.as_ref(),
            amount,
            &address,
            &asset,
            now,
        )
    })
    .map_err(|error| auth::map_db(error, None))
}

/// 宛先が未解決だった入金を、後から判明した利用者へ振り替える（controllerのみ）。
///
/// `credit` がsuspenseへ計上したイベントだけを対象にする（既に本人へ計上済みの
/// イベントを再計上しない）。同一イベントの二重請求は仕訳の要求IDで拒否する。
#[ic_cdk::update]
fn claim_unmatched_deposit(
    event_id: api_types::Blob,
    user_id: api_types::Blob,
) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can claim unmatched deposits".to_string(),
        });
    }
    let event_id: [u8; 32] = event_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "event_id must be 32 bytes".to_string(),
        })?;
    let user_id: [u8; 32] = user_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "user_id must be 32 bytes".to_string(),
        })?;
    let now = clock::now_ms();
    let network = environment::network_name()?;
    db::tx::update(|connection| {
        let event = db::repo::events::find_external_event(connection, &network, &event_id)?
            .ok_or(db::error::Error::NotFound)?;
        let kind = db::repo::ledger::journal_kind_by_external_event(connection, &event_id)?
            .ok_or(db::error::Error::Invariant("event was not credited"))?;
        if kind != "deposit_unmatched" {
            return Err(db::error::Error::Invariant(
                "event is not an unmatched deposit",
            ));
        }
        db::repo::ledger::claim_unmatched_deposit(
            connection,
            &user_id,
            event.amount,
            now,
            &event_id,
        )?;
        db::repo::events::insert_audit(
            connection,
            "controller",
            "claim_unmatched_deposit",
            None,
            Some(&hex::encode(user_id)),
            now,
        )
    })
    .map_err(|error| auth::map_db(error, None))
}

/// 入金先（準備口座）を用意する。`get_funding_instructions` の前提を作る。
#[ic_cdk::update]
async fn provision_reserve_account(session: SessionHandle) -> Result<api_types::Blob, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    let now = clock::now_ms();
    let address = outbox::provision_reserve_account(&verified.user_id, now).await?;
    Ok(address.to_vec().into())
}

/// 取引所の入金を取得して取り込む（controllerのみ）。
///
/// 取得はreplicated outcall（変換関数で決定論化）、取り込みは検証済みの`deposits::credit`。
#[ic_cdk::update]
async fn reconcile_deposits(address: api_types::Blob) -> Result<u32, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can reconcile deposits".to_string(),
        });
    }
    let address: [u8; 20] = address
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "address must be 20 bytes".to_string(),
        })?;
    let body = deposits::fetch_ledger_updates(&format!("0x{}", hex::encode(address))).await?;
    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(&body).map_err(|_| ErrorCode::UpstreamRejected {
            code: "unexpected info response".to_string(),
            retryable: false,
        })?;
    let now = clock::now_ms();
    let network = environment::network_name()?;
    let mut credited = 0;
    for entry in entries {
        let Some(hash) = entry.get("hash").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(usdc) = entry.get("usdc").and_then(|value| value.as_str()) else {
            continue;
        };
        // 負値・ゼロ・非十進は入金ではない（送金・出金など）ため読み飛ばす。
        let Some(amount) = deposits::deposit_amount_micros(usdc) else {
            continue;
        };
        let hash = hash.strip_prefix("0x").unwrap_or(hash);
        let Ok(tx_hash) = hex::decode(hash) else {
            continue;
        };
        let at = entry
            .get("time")
            .and_then(|value| value.as_u64())
            .unwrap_or(now);
        let inserted = db::tx::update(|connection| {
            deposits::credit(connection, &network, &tx_hash, amount, &address, "usdc", at)
        })
        .map_err(|error| auth::map_db(error, None))?;
        if inserted {
            credited += 1;
        }
    }
    Ok(credited)
}

/// 不明なactionを「未実行」として解消する（controllerのみ）。
///
/// 取引所が実行済みと確認できた場合の消込は、証跡（tx）を伴う別経路で行うため
/// ここでは受け付けない（`OperationNotAvailable`）。`unknown` と `dispatching` の
/// どちらも対象にするが、**取引所へ照会した証跡**を `evidence` として必須にする。
#[ic_cdk::update]
fn resolve_unknown_action(
    action_id: api_types::Blob,
    executed: bool,
    evidence: String,
) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can resolve unknown actions".to_string(),
        });
    }
    if executed {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        });
    }
    if evidence.trim().is_empty() {
        return Err(ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MissingField,
            detail: "evidence is required to resolve an action as not executed".to_string(),
        });
    }
    let action_id: [u8; 32] = action_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "action_id must be 32 bytes".to_string(),
        })?;
    let now = clock::now_ms();
    db::tx::update(|connection| {
        let (user_id, request_id, epoch, kind) =
            db::repo::actions::action_owner(connection, &action_id)?
                .ok_or(db::error::Error::NotFound)?;
        let state = db::repo::actions::action_state(connection, &action_id)?
            .ok_or(db::error::Error::NotFound)?;
        // `unknown`（応答が不明）に加えて `dispatching`（POST直前に停止し、送信の有無を
        // 自動では判定できない）も、運用者が取引所へ照会して未実行を確認した場合だけ
        // 解消できる。`executed = true` を受け付けないのは従来どおり。
        if state != api_types::fund::ActionState::Unknown
            && state != api_types::fund::ActionState::Dispatching
        {
            return Err(db::error::Error::Invariant(
                "action is neither unknown nor dispatching",
            ));
        }
        let request_id = request_id.ok_or(db::error::Error::Invariant("action without request"))?;
        let request = db::repo::funds::fund_request(connection, &user_id, &request_id)?
            .ok_or(db::error::Error::NotFound)?;
        db::repo::funds::release_reservation(connection, &user_id, &request_id, now)?;
        if kind == "withdrawal" {
            db::repo::ledger::withdrawal_release(
                connection,
                &user_id,
                request.amount,
                now,
                &request_id,
            )?;
        }
        db::repo::funds::set_request_state(
            connection,
            &user_id,
            &request_id,
            api_types::fund::FundRequestState::Rejected,
            now,
        )?;
        db::repo::actions::mark_resolved(connection, &action_id, epoch, now)?;
        db::repo::events::insert_audit(
            connection,
            "controller",
            "resolve_unknown_action",
            None,
            Some(&format!("not_executed: {evidence}")),
            now,
        )
    })
    .map_err(|error| auth::map_db(error, None))
}

/// 回収（trading口座→準備口座）を要求する。
#[ic_cdk::update]
async fn request_recovery(
    session: SessionHandle,
    client_request_id: api_types::Blob,
    amount: u64,
) -> Result<api_types::fund::FundRequestAccepted, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    fund::request_recovery(&verified, client_request_id.as_ref(), amount).await
}

fn init_db() {
    if let Err(error) = db::init(MEMORY_ID, db::schema::vault::MIGRATIONS) {
        ic_cdk::trap(format!("db init failed: {error}"));
    }
}

#[ic_cdk::init]
fn init() {
    init_db();
    // 試験ビルドでは自動sweepを組まない（PocketICの時刻前進で、試験が待つoutcallと
    // 取り違えるため）。同じsweepを`test_sweep_now`で決定的に駆動する。
    #[cfg(not(feature = "test-venue"))]
    schedule_sweep();
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    init_db();
    // グローバルtimerはアップグレードで失われるため予約し直す。
    #[cfg(not(feature = "test-venue"))]
    schedule_sweep();
}

ic_cdk::export_candid!();
