//! Zero-charge builder fee consent and approval simulation for local/testnet.
//! No `approveBuilderFee` or paid `builder` field is sent to Hyperliquid.

use api_types::auth::SessionHandle;
use api_types::builder_fee::{BuilderFeeConsent, BuilderFeeConsentClaims, BuilderFeeMockStatus};
use api_types::error::{BadRequestCode, ErrorCode};
use api_types::{AccountKind, Blob, Network};
use candid::Principal;

const MAX_VALIDITY_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const CLOCK_SKEW_MS: u64 = 5 * 60 * 1000;

fn malformed() -> ErrorCode {
    ErrorCode::BadRequest {
        code: BadRequestCode::MalformedPayload,
        detail: "invalid builder fee consent".into(),
    }
}

fn domain_digest(claims: &BuilderFeeConsentClaims) -> Result<[u8; 32], ErrorCode> {
    let encoded = candid::encode_one(claims).map_err(|_| malformed())?;
    Ok(hl_sign::keccak256_concat(&[
        b"private-perp/builder-fee-consent/v1",
        &encoded,
    ]))
}

fn personal_digest(message: &[u8; 32]) -> [u8; 32] {
    hl_sign::keccak256_concat(&[b"\x19Ethereum Signed Message:\n32", message])
}

fn require_non_mainnet() -> Result<Network, ErrorCode> {
    let network = crate::environment::network()?;
    if network == Network::Mainnet {
        Err(ErrorCode::PolicyUnavailable)
    } else {
        Ok(network)
    }
}

pub async fn signing_claims(
    session: &SessionHandle,
    builder_address: Blob,
    expires_at: u64,
) -> Result<(BuilderFeeConsentClaims, Blob), ErrorCode> {
    let network = require_non_mainnet()?;
    let caller = ic_cdk::api::msg_caller();
    let verified = crate::auth::verify_session(session, caller)?;
    let builder: [u8; 20] = builder_address
        .as_ref()
        .try_into()
        .map_err(|_| malformed())?;
    let now = crate::clock::now_ms();
    if builder == [0; 20] || expires_at <= now || expires_at > now.saturating_add(MAX_VALIDITY_MS) {
        return Err(malformed());
    }
    let account = crate::outbox::ensure_trading_account(&verified.user_id, now).await?;
    crate::auth::verify_session(session, caller)?;
    let nonce = crate::random::random32().await?;
    crate::auth::verify_session(session, caller)?;
    if require_non_mainnet()? != network {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let claims = BuilderFeeConsentClaims {
        principal: caller,
        user_id: verified.user_id.to_vec().into(),
        account_id: account.account_id.to_vec().into(),
        vault: ic_cdk::api::canister_self(),
        network,
        builder_address,
        fee_decibps: 0,
        issued_at: crate::clock::now_ms(),
        expires_at,
        nonce: nonce.to_vec().into(),
    };
    let digest = domain_digest(&claims)?;
    Ok((claims, digest.to_vec().into()))
}

pub fn register(
    session: &SessionHandle,
    consent: BuilderFeeConsent,
) -> Result<BuilderFeeMockStatus, ErrorCode> {
    let network = require_non_mainnet()?;
    let caller = ic_cdk::api::msg_caller();
    let verified = crate::auth::verify_session(session, caller)?;
    let claims = &consent.claims;
    let now = crate::clock::now_ms();
    let account_id: [u8; 32] = claims
        .account_id
        .as_ref()
        .try_into()
        .map_err(|_| malformed())?;
    let nonce: [u8; 32] = claims.nonce.as_ref().try_into().map_err(|_| malformed())?;
    let builder: [u8; 20] = claims
        .builder_address
        .as_ref()
        .try_into()
        .map_err(|_| malformed())?;
    if claims.principal != caller
        || claims.vault != ic_cdk::api::canister_self()
        || claims.user_id.as_ref() != verified.user_id
        || claims.network != network
        || builder == [0; 20]
        || claims.fee_decibps != 0
        || claims.issued_at > now.saturating_add(CLOCK_SKEW_MS)
        || claims.expires_at <= now
        || claims.expires_at <= claims.issued_at
        || claims.expires_at - claims.issued_at > MAX_VALIDITY_MS
    {
        return Err(malformed());
    }
    let account = db::tx::query(|c| {
        db::repo::ledger::custody_account(c, &verified.user_id, AccountKind::Trading)
    })
    .map_err(|error| crate::auth::map_db(error, None))?;
    if account.as_ref().map(|row| row.account_id) != Some(account_id) {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        });
    }
    let signature_bytes: [u8; 65] = consent
        .eoa_signature
        .as_ref()
        .try_into()
        .map_err(|_| malformed())?;
    let signature = hl_sign::Signature::from_bytes65(&signature_bytes).map_err(|_| malformed())?;
    let digest = domain_digest(claims)?;
    let recovered = hl_sign::recover_address(&personal_digest(&digest), &signature, None)
        .map_err(|_| malformed())?;
    let identity = db::tx::query(|c| db::repo::auth::identity_by_user(c, &verified.user_id))
        .map_err(|error| crate::auth::map_db(error, None))?
        .ok_or(ErrorCode::Unauthenticated {
            reason: "identity missing".into(),
        })?;
    if recovered != identity.eoa_address {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::InvalidSignature,
            detail: "builder fee consent signer mismatch".into(),
        });
    }
    db::tx::update(|c| {
        db::repo::builder_fee_mock::register(
            c,
            &verified.user_id,
            caller.as_slice(),
            &account_id,
            &builder,
            hl_types::environment::network_name(network.into()),
            claims.issued_at,
            claims.expires_at,
            &nonce,
            &digest,
            &signature_bytes,
            now,
        )
    })
    .map_err(|error| crate::auth::map_db(error, None))?;
    status(&verified.user_id, caller, &account_id)
}

pub fn status(
    user_id: &[u8; 32],
    principal: Principal,
    account_id: &[u8; 32],
) -> Result<BuilderFeeMockStatus, ErrorCode> {
    require_non_mainnet()?;
    let row = db::tx::query(|c| db::repo::builder_fee_mock::consent(c, user_id))
        .map_err(|error| crate::auth::map_db(error, None))?;
    let (approval_records, charged) =
        db::tx::query(|c| db::repo::builder_fee_mock::accounting(c, user_id))
            .map_err(|error| crate::auth::map_db(error, None))?;
    let approved = row.as_ref().is_some_and(|row| {
        row.principal == principal.as_slice()
            && row.account_id == *account_id
            && row.network == crate::environment::network_name().unwrap_or_default()
            && row.expires_at > crate::clock::now_ms()
    });
    Ok(BuilderFeeMockStatus {
        approved,
        builder_address: row.as_ref().map(|row| row.builder_address.to_vec().into()),
        expires_at: row.map(|row| row.expires_at),
        approval_records,
        charged_micros: charged,
    })
}
