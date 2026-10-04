//! Synthetic eligibility admission. The issuer secret never enters this canister.

use api_types::auth::SessionHandle;
use api_types::eligibility::{EligibilityClaims, EligibilityStatus, EligibilityToken};
use api_types::error::{BadRequestCode, ErrorCode};
use candid::Principal;

const MAX_VALIDITY_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const CLOCK_SKEW_MS: u64 = 5 * 60 * 1000;

fn malformed() -> ErrorCode {
    ErrorCode::BadRequest {
        code: BadRequestCode::MalformedPayload,
        detail: "invalid eligibility token".into(),
    }
}

pub fn digest(claims: &EligibilityClaims) -> Result<[u8; 32], ErrorCode> {
    let encoded = candid::encode_one(claims).map_err(|_| malformed())?;
    Ok(hl_sign::keccak256_concat(&[
        b"private-perp/eligibility/v1",
        &encoded,
    ]))
}

pub fn configure(
    terms_version: u64,
    issuer_address: api_types::Blob,
    mock_issuer: bool,
) -> Result<(), ErrorCode> {
    journal_client::require_management()?;
    let issuer: [u8; 20] = issuer_address
        .as_ref()
        .try_into()
        .map_err(|_| malformed())?;
    if issuer == [0; 20] || terms_version == 0 {
        return Err(malformed());
    }
    if crate::environment::network()? == api_types::Network::Mainnet && mock_issuer {
        return Err(ErrorCode::PolicyUnavailable);
    }
    if let Some(current) =
        db::tx::query(db::repo::eligibility::config).map_err(|e| crate::auth::map_db(e, None))?
    {
        // A key rotation must invalidate tokens issued by the old key. The
        // stored token only carries the terms version, so require a bump.
        if terms_version < current.terms_version
            || (issuer != current.issuer_address && terms_version == current.terms_version)
        {
            return Err(ErrorCode::PolicyUnavailable);
        }
    }
    db::tx::update(|c| db::repo::eligibility::configure(c, terms_version, &issuer, mock_issuer))
        .map_err(|e| crate::auth::map_db(e, None))
}

pub async fn signing_claims(
    session: &SessionHandle,
    expires_at: u64,
) -> Result<EligibilityClaims, ErrorCode> {
    let verified = crate::auth::verify_session(session, ic_cdk::api::msg_caller())?;
    let config = db::tx::query(db::repo::eligibility::config)
        .map_err(|e| crate::auth::map_db(e, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    let now = crate::clock::now_ms();
    if expires_at <= now || expires_at > now.saturating_add(MAX_VALIDITY_MS) {
        return Err(malformed());
    }
    let account = crate::outbox::ensure_trading_account(&verified.user_id, now).await?;
    // Await above may have changed session or config. Re-validate before returning the target.
    crate::auth::verify_session(session, ic_cdk::api::msg_caller())?;
    let current = db::tx::query(db::repo::eligibility::config)
        .map_err(|e| crate::auth::map_db(e, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    if current.terms_version != config.terms_version {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let nonce = crate::random::random32().await?;
    crate::auth::verify_session(session, ic_cdk::api::msg_caller())?;
    let current = db::tx::query(db::repo::eligibility::config)
        .map_err(|e| crate::auth::map_db(e, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    if current.terms_version != config.terms_version {
        return Err(ErrorCode::PolicyUnavailable);
    }
    Ok(EligibilityClaims {
        principal: ic_cdk::api::msg_caller(),
        user_id: verified.user_id.to_vec().into(),
        account_id: account.account_id.to_vec().into(),
        network: crate::environment::network()?,
        vault: ic_cdk::api::canister_self(),
        terms_version: config.terms_version,
        issued_at: crate::clock::now_ms(),
        expires_at,
        nonce: nonce.to_vec().into(),
    })
}

pub fn register(
    session: &SessionHandle,
    token: EligibilityToken,
) -> Result<EligibilityStatus, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    let verified = crate::auth::verify_session(session, caller)?;
    let claims = &token.claims;
    let now = crate::clock::now_ms();
    let network = crate::environment::network()?;
    if network == api_types::Network::Mainnet {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let config = db::tx::query(db::repo::eligibility::config)
        .map_err(|e| crate::auth::map_db(e, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    if claims.principal != caller
        || claims.vault != ic_cdk::api::canister_self()
        || claims.user_id.as_ref() != verified.user_id
        || claims.network != network
        || claims.terms_version != config.terms_version
        || claims.issued_at > now.saturating_add(CLOCK_SKEW_MS)
        || claims.expires_at <= now
        || claims.expires_at <= claims.issued_at
        || claims.expires_at - claims.issued_at > MAX_VALIDITY_MS
    {
        return Err(ErrorCode::NotEligible {
            policy_version: config.terms_version,
        });
    }
    let account_id: [u8; 32] = claims
        .account_id
        .as_ref()
        .try_into()
        .map_err(|_| malformed())?;
    let nonce: [u8; 32] = claims.nonce.as_ref().try_into().map_err(|_| malformed())?;
    let owned = db::tx::query(|c| {
        db::repo::ledger::custody_account(c, &verified.user_id, api_types::AccountKind::Trading)
    })
    .map_err(|e| crate::auth::map_db(e, None))?;
    if owned.as_ref().map(|a| a.account_id) != Some(account_id) {
        return Err(ErrorCode::NotEligible {
            policy_version: config.terms_version,
        });
    }
    let signature_bytes: [u8; 65] = token
        .signature
        .as_ref()
        .try_into()
        .map_err(|_| malformed())?;
    let signature = hl_sign::Signature::from_bytes65(&signature_bytes).map_err(|_| malformed())?;
    let digest = digest(claims)?;
    let address = hl_sign::recover_address(&digest, &signature, None).map_err(|_| malformed())?;
    if address != config.issuer_address {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::InvalidSignature,
            detail: "eligibility issuer mismatch".into(),
        });
    }
    db::tx::update(|c| {
        db::repo::eligibility::register(
            c,
            &verified.user_id,
            caller.as_slice(),
            &account_id,
            hl_types::environment::network_name(network.into()),
            claims.terms_version,
            claims.issued_at,
            claims.expires_at,
            &nonce,
            &digest,
        )
    })
    .map_err(|e| crate::auth::map_db(e, None))?;
    Ok(EligibilityStatus {
        terms_version: config.terms_version,
        expires_at: Some(claims.expires_at),
        eligible: true,
    })
}

pub fn status(
    user_id: &[u8; 32],
    principal: Principal,
    account_id: &[u8; 32],
) -> Result<EligibilityStatus, ErrorCode> {
    let config = db::tx::query(db::repo::eligibility::config)
        .map_err(|e| crate::auth::map_db(e, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    if crate::environment::network()? == api_types::Network::Mainnet {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let row = db::tx::query(|c| db::repo::eligibility::token(c, user_id))
        .map_err(|e| crate::auth::map_db(e, None))?;
    let eligible = row.as_ref().is_some_and(|row| {
        row.principal == principal.as_slice()
            && row.account_id == *account_id
            && row.network == crate::environment::network_name().unwrap_or_default()
            && row.terms_version == config.terms_version
            && row.expires_at > crate::clock::now_ms()
    });
    Ok(EligibilityStatus {
        terms_version: config.terms_version,
        expires_at: row.map(|row| row.expires_at),
        eligible,
    })
}

pub fn require(
    user_id: &[u8; 32],
    principal: Principal,
    account_id: &[u8; 32],
) -> Result<(), ErrorCode> {
    let status = status(user_id, principal, account_id)?;
    if status.eligible {
        Ok(())
    } else {
        Err(ErrorCode::NotEligible {
            policy_version: status.terms_version,
        })
    }
}

/// Worker-side check after awaits; the account and registered principal must still match.
pub fn require_current(user_id: &[u8; 32], account_id: &[u8; 32]) -> Result<(), ErrorCode> {
    let row = db::tx::query(|c| db::repo::eligibility::token(c, user_id))
        .map_err(|e| crate::auth::map_db(e, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    let owned = db::tx::query(|c| {
        db::repo::ledger::custody_account(c, user_id, api_types::AccountKind::Trading)
    })
    .map_err(|e| crate::auth::map_db(e, None))?;
    if owned.as_ref().map(|a| a.account_id) != Some(*account_id) {
        return Err(ErrorCode::PolicyUnavailable);
    }
    require(user_id, Principal::from_slice(&row.principal), account_id)
}
