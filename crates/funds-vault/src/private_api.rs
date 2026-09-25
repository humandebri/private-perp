//! 個人向け書込みのHPKE受付。本文と結果は公開Candidへ出さない。

use api_types::envelope::{HpkeRequest, HpkeResponse};
use api_types::error::{BadRequestCode, ErrorCode};
use candid::{CandidType, Principal};

const ENVELOPE_INFO: &[u8] = b"private-perp/envelope/v1";
const MAX_TTL_MS: u64 = 60_000;
const MAX_CIPHERTEXT: usize = 64 * 1024;

fn malformed(detail: &str) -> ErrorCode {
    ErrorCode::BadRequest {
        code: BadRequestCode::MalformedPayload,
        detail: detail.into(),
    }
}

pub async fn open(envelope: &HpkeRequest) -> Result<(Vec<u8>, [u8; 32], Principal), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    let now = crate::clock::now_ms();
    let public = db::tx::query(db::repo::hpke::active_public)
        .map_err(|e| crate::auth::map_db(e, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    if envelope.key_id.as_ref() != public.as_slice()
        || envelope.canister != ic_cdk::api::canister_self()
        || envelope.client_public_key.len() != 32
        || envelope.ciphertext.len() > MAX_CIPHERTEXT
    {
        return Err(malformed("invalid HPKE envelope"));
    }
    let network = crate::environment::network_name()?;
    if hl_types::environment::network_name(envelope.network.into()) != network {
        return Err(malformed("envelope network mismatch"));
    }
    if envelope.expires_at <= now || envelope.expires_at > now.saturating_add(MAX_TTL_MS) {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::ExpiredIntent,
            detail: "envelope expired".into(),
        });
    }
    let request_id: [u8; 32] = envelope
        .request_id
        .as_ref()
        .try_into()
        .map_err(|_| malformed("request_id must be 32 bytes"))?;
    let aad = hpke_envelope::envelope_aad(
        &network,
        ic_cdk::api::canister_self().as_slice(),
        &envelope.method,
        caller.as_slice(),
        &request_id,
        envelope.expires_at,
    );
    if envelope.aad.as_ref() != aad.as_slice() {
        return Err(malformed("envelope AAD mismatch"));
    }
    let secret = db::tx::query(db::repo::hpke::active_secret)
        .map_err(|e| crate::auth::map_db(e, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    let plaintext = hpke_envelope::open(&secret, ENVELOPE_INFO, &aad, envelope.ciphertext.as_ref())
        .map_err(|_| malformed("cannot open envelope"))?;
    let consumed = db::tx::update(|c| {
        db::repo::hpke_requests::consume(
            c,
            &request_id,
            &envelope.method,
            caller.as_slice(),
            now,
            envelope.expires_at,
        )
    })
    .map_err(|e| crate::auth::map_db(e, None))?;
    if !consumed {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::NonceReused,
            detail: "request_id reused".into(),
        });
    }
    Ok((plaintext, request_id, caller))
}

pub async fn seal<T: CandidType>(
    envelope: &HpkeRequest,
    request_id: &[u8; 32],
    caller: Principal,
    result: &T,
) -> Result<HpkeResponse, ErrorCode> {
    let network = crate::environment::network_name()?;
    let aad = hpke_envelope::envelope_aad(
        &network,
        ic_cdk::api::canister_self().as_slice(),
        &envelope.method,
        caller.as_slice(),
        request_id,
        envelope.expires_at,
    );
    let plaintext = candid::encode_one(result).map_err(|e| ErrorCode::Internal {
        code: e.to_string(),
    })?;
    let seed = crate::random::random32().await?;
    let ciphertext = hpke_envelope::seal(
        envelope.client_public_key.as_ref(),
        ENVELOPE_INFO,
        &aad,
        &plaintext,
        &seed,
    )
    .map_err(|_| malformed("cannot seal response"))?;
    Ok(HpkeResponse {
        request_id: request_id.to_vec().into(),
        key_id: envelope.key_id.clone(),
        observed_at: crate::clock::now_ms(),
        ciphertext: ciphertext.into(),
    })
}
