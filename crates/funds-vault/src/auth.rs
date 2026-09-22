//! EOA認証（challengeとセッション）。`Plan.md` 16.1、`docs/phase-0/api-contract.md` 2.1。
//!
//! 署名は `hl_sign::private_perp` の独自EIP-712型で検証する。challengeは5分・1回限り、
//! セッションは30分で、失効世代が変わると無効になる。

use crate::{clock, config, random};
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
    SessionStatus,
};
use api_types::error::{BadRequestCode, ErrorCode, NotAllowedCode};
use candid::Principal;
use db::error::Error as DbError;
use db::repo::auth::{ChallengeRow, SessionRow};
use hl_sign::private_perp;
use hl_sign::signature::{Signature, recover_address};

/// 検証済みセッション。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSession {
    pub user_id: [u8; 32],
    pub session_id: [u8; 32],
}

fn bad(code: BadRequestCode, detail: &str) -> ErrorCode {
    ErrorCode::BadRequest {
        code,
        detail: detail.to_string(),
    }
}

/// ドメインのエラーをAPIのエラーへ写す。
pub fn map_db(error: DbError, request_id: Option<&[u8]>) -> ErrorCode {
    match error {
        DbError::InsufficientFunds {
            available,
            requested,
        } => ErrorCode::InsufficientFunds {
            available: u64::try_from(available).unwrap_or(0),
            requested: u64::try_from(requested).unwrap_or(0),
        },
        DbError::RiskLimitExceeded { limit } => ErrorCode::RiskLimitExceeded { limit },
        DbError::Conflict => match request_id {
            Some(request_id) => ErrorCode::IdempotencyConflict {
                request_id: request_id.to_vec().into(),
            },
            None => ErrorCode::NotAllowed {
                code: NotAllowedCode::OperationNotAvailable,
            },
        },
        DbError::StateConflict { .. } => ErrorCode::ReservationConflict,
        DbError::Overflow => bad(BadRequestCode::QuantityOutOfRange, "integer overflow"),
        DbError::Invariant(message) => bad(BadRequestCode::MalformedPayload, message),
        DbError::NotFound => ErrorCode::Unauthenticated {
            reason: "not found".to_string(),
        },
        DbError::Sql(message) => ErrorCode::Internal { code: message },
    }
}

fn signature_error(error: hl_sign::SignError) -> ErrorCode {
    bad(BadRequestCode::InvalidSignature, &error.to_string())
}

const fn purpose_name(purpose: ChallengePurpose) -> &'static str {
    match purpose {
        ChallengePurpose::Login => "login",
        ChallengePurpose::Withdrawal => "withdrawal",
    }
}

fn to_fixed<const N: usize>(bytes: &[u8], what: &str) -> Result<[u8; N], ErrorCode> {
    bytes
        .try_into()
        .map_err(|_| bad(BadRequestCode::MalformedPayload, what))
}

fn build_challenge(
    challenge_id_row: &ChallengeRow,
    canister: Principal,
) -> private_perp::Challenge {
    private_perp::Challenge {
        purpose: challenge_id_row.purpose.clone(),
        eoa: challenge_id_row.eoa_address,
        principal: challenge_id_row.principal.clone(),
        canister: canister.as_slice().to_vec(),
        network: challenge_id_row.network.clone(),
        origin: challenge_id_row.origin.clone(),
        nonce: challenge_id_row.nonce,
        expires_at: challenge_id_row.expires_at,
    }
}

/// クライアントが署名するEIP-712 typed data（JSON）。
fn typed_data_json(challenge: &private_perp::Challenge) -> Result<Vec<u8>, ErrorCode> {
    let domain = private_perp::domain();
    let types: Vec<serde_json::Value> = private_perp::CHALLENGE_FIELDS
        .iter()
        .map(|field| {
            serde_json::json!({
                "name": field.name,
                "type": field.kind.as_str(),
            })
        })
        .collect();
    let message = serde_json::json!({
        "purpose": challenge.purpose,
        "eoa": format!("0x{}", hex::encode(challenge.eoa)),
        "principal": format!("0x{}", hex::encode(&challenge.principal)),
        "canister": format!("0x{}", hex::encode(&challenge.canister)),
        "network": challenge.network,
        "origin": challenge.origin,
        "nonce": format!("0x{}", hex::encode(challenge.nonce)),
        "expiresAt": challenge.expires_at,
    });
    let domain_types = serde_json::json!([
        { "name": "name", "type": "string" },
        { "name": "version", "type": "string" },
        { "name": "chainId", "type": "uint256" },
        { "name": "verifyingContract", "type": "address" },
    ]);
    let typed_data = serde_json::json!({
        "domain": {
            "name": domain.name,
            "version": domain.version,
            "chainId": domain.chain_id,
            "verifyingContract": "0x0000000000000000000000000000000000000000",
        },
        "types": {
            "EIP712Domain": domain_types,
            private_perp::CHALLENGE_PRIMARY_TYPE: types,
        },
        "primaryType": private_perp::CHALLENGE_PRIMARY_TYPE,
        "message": message,
    });
    serde_json::to_vec(&typed_data).map_err(|error| ErrorCode::Internal {
        code: error.to_string(),
    })
}

/// challengeを発行する。発行レートの上限を超えた場合は拒否する。
///
/// challengeは**呼び出し元のprincipal**へ束縛する。クライアントが申告したprincipalを
/// そのまま保存すると、他人が取得した署名を別principalでredeemできてしまう
/// （`threat-test-matrix.md` T-102）。
pub async fn issue_challenge(
    request: ChallengeRequest,
    canister: Principal,
) -> Result<ChallengeResponse, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if caller == Principal::anonymous() {
        return Err(ErrorCode::Unauthenticated {
            reason: "anonymous caller cannot issue a challenge".to_string(),
        });
    }
    if request.principal != caller {
        return Err(ErrorCode::Unauthenticated {
            reason: "challenge principal must be the caller".to_string(),
        });
    }
    if request.origin.is_empty() {
        return Err(bad(BadRequestCode::MissingField, "origin"));
    }
    if request.origin.len() > 256 {
        return Err(bad(BadRequestCode::TooLarge, "origin"));
    }
    let eoa_address = to_fixed::<20>(&request.eoa_address, "eoa_address must be 20 bytes")?;

    let now = clock::now_ms();
    let challenge_id = random::random32().await?;
    let nonce = random::random32().await?;
    let expires_at = now.saturating_add(config::CHALLENGE_TTL_MS);

    let challenge = private_perp::Challenge {
        purpose: purpose_name(request.purpose).to_string(),
        eoa: eoa_address,
        principal: request.principal.as_slice().to_vec(),
        canister: canister.as_slice().to_vec(),
        network: config::network_name(request.network).to_string(),
        origin: request.origin.clone(),
        nonce,
        expires_at,
    };
    let typed_data = typed_data_json(&challenge)?;

    let row = ChallengeRow {
        challenge_id,
        nonce,
        eoa_address,
        principal: caller.as_slice().to_vec(),
        purpose: purpose_name(request.purpose).to_string(),
        network: config::network_name(request.network).to_string(),
        origin: request.origin,
        expires_at,
        consumed_at: None,
    };

    db::tx::update(|connection| {
        // 期限切れ・使用済みの行を掃除してから発行する（行の無制限な増加を防ぐ）。
        db::repo::auth::delete_expired_challenges(connection, now)?;
        let since = now.saturating_sub(config::CHALLENGE_RATE_WINDOW_MS);
        let recent = db::repo::auth::count_recent_challenges(connection, &eoa_address, since)?;
        if recent >= config::CHALLENGE_RATE_LIMIT {
            return Err(DbError::Invariant("challenge rate limit exceeded"));
        }
        db::repo::auth::insert_challenge(connection, &row, now)
    })
    .map_err(|error| map_db(error, None))?;

    Ok(ChallengeResponse {
        challenge_id: challenge_id.to_vec().into(),
        typed_data: typed_data.into(),
        nonce: nonce.to_vec().into(),
        expires_at,
    })
}

/// challengeを消費してセッションを発行する。
///
/// challengeが束縛されたprincipalと呼び出し元が一致することを要求する（T-102）。
/// 一致を確認しないと、他人が取得した署名を自分のprincipalでredeemしてセッションを
/// 得られてしまう。
pub async fn open_session(
    request: OpenSessionRequest,
    caller: Principal,
) -> Result<SessionHandle, ErrorCode> {
    if caller == Principal::anonymous() {
        return Err(ErrorCode::Unauthenticated {
            reason: "anonymous caller cannot open a session".to_string(),
        });
    }
    let challenge_id = to_fixed::<32>(&request.challenge_id, "challenge_id must be 32 bytes")?;
    let signature_bytes = to_fixed::<65>(&request.eoa_signature, "eoa_signature must be 65 bytes")?;
    let now = clock::now_ms();

    // 束縛されたprincipalを**消費の前**に確認する（他人がchallengeを消費して正規の
    // ログインを妨害できないようにする。T-102）。
    let owner =
        db::tx::query(|connection| db::repo::auth::challenge_row(connection, &challenge_id))
            .map_err(|error| map_db(error, None))?
            .ok_or_else(|| bad(BadRequestCode::ChallengeExpired, "unknown challenge"))?;
    if owner.principal != caller.as_slice() {
        return Err(ErrorCode::Unauthenticated {
            reason: "challenge was issued for another principal".to_string(),
        });
    }

    let row = db::tx::update(|connection| {
        db::repo::auth::consume_challenge(connection, &challenge_id, now)
    })
    .map_err(|error| match error {
        DbError::Conflict => bad(BadRequestCode::ChallengeReused, "challenge already used"),
        DbError::Invariant(message) if message.contains("expired") => {
            bad(BadRequestCode::ChallengeExpired, "challenge expired")
        }
        other => map_db(other, Some(&challenge_id)),
    })?;

    if row.purpose != purpose_name(ChallengePurpose::Login) {
        return Err(bad(
            BadRequestCode::MalformedPayload,
            "challenge purpose is not login",
        ));
    }

    let challenge = build_challenge(&row, ic_cdk::api::canister_self());
    let digest = challenge.digest().map_err(signature_error)?;
    let signature = Signature::from_bytes65(&signature_bytes).map_err(signature_error)?;
    let recovered = recover_address(&digest, &signature, None).map_err(signature_error)?;
    if recovered != row.eoa_address {
        return Err(bad(
            BadRequestCode::InvalidSignature,
            "signature does not match the EOA",
        ));
    }

    let session_id = random::random32().await?;
    let candidate_user_id = random::random32().await?;
    let expires_at = now.saturating_add(config::SESSION_TTL_MS);

    let (_user_id, revocation_generation) = db::tx::update(|connection| {
        let identity =
            db::repo::auth::ensure_identity(connection, &row.eoa_address, &candidate_user_id, now)?;
        db::repo::auth::touch_login(connection, &identity.user_id, now)?;
        db::repo::auth::create_session(
            connection,
            &session_id,
            &identity.user_id,
            caller.as_slice(),
            now,
            expires_at,
            identity.revocation_generation,
        )?;
        db::repo::events::insert_audit(
            connection,
            "user",
            "open_session",
            None,
            Some("login"),
            now,
        )?;
        Ok((identity.user_id, identity.revocation_generation))
    })
    .map_err(|error| map_db(error, None))?;

    Ok(SessionHandle {
        session_id: session_id.to_vec().into(),
        vault_principal: ic_cdk::api::canister_self(),
        expires_at,
        revocation_generation,
    })
}

/// セッションを検証する。期限・失効・世代・caller束縛を確認する。
pub fn verify_session(
    session: &SessionHandle,
    caller: Principal,
) -> Result<VerifiedSession, ErrorCode> {
    let session_id = to_fixed::<32>(&session.session_id, "session_id must be 32 bytes")?;
    let now = clock::now_ms();
    let (valid, stored) = db::tx::query(|connection| {
        let valid = db::repo::auth::find_valid_session(connection, &session_id, now)?;
        let stored = db::repo::auth::session_row(connection, &session_id)?;
        Ok((valid, stored))
    })
    .map_err(|error| map_db(error, None))?;

    let row: SessionRow = match (valid, stored) {
        (Some(row), _) => row,
        (None, Some(row)) if row.revoked_at.is_some() => return Err(ErrorCode::SessionRevoked),
        (None, Some(_)) => return Err(ErrorCode::SessionExpired),
        (None, None) => {
            return Err(ErrorCode::Unauthenticated {
                reason: "unknown session".to_string(),
            });
        }
    };

    if row.principal != caller.as_slice() {
        return Err(ErrorCode::Unauthenticated {
            reason: "session does not belong to this caller".to_string(),
        });
    }
    if row.expires_at != session.expires_at {
        return Err(ErrorCode::SessionExpired);
    }
    if row.revocation_generation != session.revocation_generation {
        return Err(ErrorCode::SessionRevoked);
    }

    Ok(VerifiedSession {
        user_id: row.user_id,
        session_id,
    })
}

/// セッションの有効性を返す（canister間の検証経路。`trading_core` が使う）。
///
/// 呼び出し元の束縛はここでは行わない。返した `principal` を呼び出し側が自分の
/// `msg_caller` と比較して認可する（vaultはcoreの`msg_caller`を知らないため）。
pub fn session_status(session: &SessionHandle) -> Result<SessionStatus, ErrorCode> {
    let session_id = to_fixed::<32>(&session.session_id, "session_id must be 32 bytes")?;
    let now = clock::now_ms();
    let (valid, stored) = db::tx::query(|connection| {
        let valid = db::repo::auth::find_valid_session(connection, &session_id, now)?;
        let stored = db::repo::auth::session_row(connection, &session_id)?;
        Ok((valid, stored))
    })
    .map_err(|error| map_db(error, None))?;

    let row: SessionRow = match (valid, stored) {
        (Some(row), _) => row,
        (None, Some(row)) if row.revoked_at.is_some() => return Err(ErrorCode::SessionRevoked),
        (None, Some(_)) => return Err(ErrorCode::SessionExpired),
        (None, None) => {
            return Err(ErrorCode::Unauthenticated {
                reason: "unknown session".to_string(),
            });
        }
    };

    let principal = Principal::from_slice(&row.principal);
    Ok(SessionStatus {
        user_id: row.user_id.to_vec().into(),
        principal,
        expires_at: row.expires_at,
        revocation_generation: row.revocation_generation,
    })
}

/// セッションを失効させる（本人のログアウト）。
pub fn revoke_session(session: &SessionHandle, caller: Principal) -> Result<(), ErrorCode> {
    let verified = verify_session(session, caller)?;
    let now = clock::now_ms();
    db::tx::update(|connection| {
        db::repo::auth::revoke_session(connection, &verified.session_id, now)
    })
    .map_err(|error| map_db(error, None))
}
