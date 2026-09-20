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
mod fund;
mod hpke;
mod outbox;
mod random;
mod venue;

use api_types::auth::{ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle};
use api_types::error::ErrorCode;
use candid::Principal;

const MEMORY_ID: u8 = db::memory_id::FUNDS_VAULT_MAIN;

thread_local! {
    /// 直近のsweep時刻（heartbeatの間隔ゲート）。
    static LAST_SWEEP: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
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

/// 現行のHPKE公開鍵。未生成はエラー（機密性の前提が欠けている）。
#[ic_cdk::query]
fn get_hpke_public_key() -> Result<api_types::Blob, ErrorCode> {
    let public =
        db::tx::query(db::repo::hpke::active_public).map_err(|error| auth::map_db(error, None))?;
    public
        .map(|public| public.into())
        .ok_or(ErrorCode::PolicyUnavailable)
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

/// Agent世代を要求する（未承認の世代があればそれを返す）。
#[ic_cdk::update]
async fn request_agent_generation(
    session: SessionHandle,
) -> Result<api_types::fund::AgentGeneration, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    fund::request_agent_generation(&verified).await
}

/// Agent世代の状態。
#[ic_cdk::query]
fn get_agent_status(session: SessionHandle) -> Result<api_types::fund::AgentStatus, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    fund::agent_status(&verified)
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
fn request_withdrawal(
    request: api_types::fund::WithdrawalRequest,
) -> Result<api_types::fund::FundRequestAccepted, ErrorCode> {
    let verified = auth::verify_session(&request.session, ic_cdk::api::msg_caller())?;
    fund::request_withdrawal(&verified, &request)
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

/// 未処理の資金actionを処理する（heartbeatから間隔を空けて呼ぶ）。
#[ic_cdk::heartbeat]
async fn heartbeat() {
    let now = clock::now_ms();
    let due = LAST_SWEEP.with(|cell| {
        let previous = cell.get();
        if now.saturating_sub(previous) < outbox::SWEEP_INTERVAL_MS {
            false
        } else {
            cell.set(now);
            true
        }
    });
    if !due {
        return;
    }
    let _ = outbox::sweep(now).await;
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

fn init_db() {
    if let Err(error) = db::init(MEMORY_ID, db::schema::vault::MIGRATIONS) {
        ic_cdk::trap(format!("db init failed: {error}"));
    }
}

#[ic_cdk::init]
fn init() {
    init_db();
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    init_db();
}

ic_cdk::export_candid!();
