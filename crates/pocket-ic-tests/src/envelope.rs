//! 封筒（HPKE）のクライアント側（試験専用）。
//!
//! `docs/phase-0/api-contract.md` 6節の個人APIを叩くための最小のクライアントである。
//! ブラウザ実装の代わりに、封筒の組み立て・`aad`の束縛・応答の復号をここで行う。

use api_types::Network;
use api_types::envelope::{HpkeRequest, HpkeResponse};
use api_types::error::ErrorCode;
use candid::{CandidType, Principal};
use pocket_ic::PocketIc;
use serde::de::DeserializeOwned;
use std::sync::atomic::{AtomicU64, Ordering};

/// 用途分離ラベル（canister側と同じ値）。
pub const ENVELOPE_INFO: &[u8] = b"private-perp/envelope/v1";

/// 封筒の既定の期限（`now`からの猶予）。
pub const DEFAULT_TTL_MS: u64 = 60_000;

/// PocketICの時刻（ミリ秒）。canisterの時刻と同じ基準（UNIX epoch）。
pub fn now_ms(pic: &PocketIc) -> u64 {
    pic.get_time().as_nanos_since_unix_epoch() / 1_000_000
}

/// 試験用の封筒クライアント（クライアント鍵と要求IDの採番を持つ）。
pub struct EnvelopeClient {
    secret: [u8; 32],
    public: [u8; 32],
    network: String,
}

/// `local`ネットワークのクライアントを作る（決定的なクライアント鍵）。
pub fn client(seed: u8) -> EnvelopeClient {
    EnvelopeClient::new(seed, "local")
}

impl EnvelopeClient {
    /// テストヘルパ用。既存の平文引数を同じ公開HPKE入口へ包んで送る。
    pub fn call_encoded<R: CandidType + DeserializeOwned>(
        &self,
        pic: &PocketIc,
        canister: Principal,
        caller: Principal,
        method: &str,
        plaintext: &[u8],
    ) -> Result<R, String> {
        let (envelope, aad) = self.prepare_encoded(pic, canister, caller, method, plaintext)?;
        let response: Result<HpkeResponse, ErrorCode> = if method == "request_recovery" {
            let (session, _, _): (api_types::auth::SessionHandle, api_types::Blob, u64) =
                candid::decode_args(plaintext).map_err(|e| e.to_string())?;
            let status: Result<api_types::fund::FundStatus, ErrorCode> =
                crate::query(pic, canister, caller, "get_fund_status", session)?;
            let equity = status.map(|s| s.trading_equity).unwrap_or(0);
            crate::call_with_routed_outcalls(pic,canister,caller,"private_call",(envelope.clone(),),|call| {
                let request: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
                assert_eq!(request["type"],"clearinghouseState");
                Ok((200,serde_json::json!({"marginSummary":{"accountValue":format!("{}.{:06}",equity/1_000_000,equity%1_000_000)},"assetPositions":[],"time":now_ms(pic)}).to_string().into_bytes()))
            })?.0
        } else {
            crate::update(pic, canister, caller, "private_call", envelope.clone())?
        };
        self.decode_encoded(response, &envelope, &aad)
    }

    pub fn prepare_encoded(
        &self,
        pic: &PocketIc,
        canister: Principal,
        caller: Principal,
        method: &str,
        plaintext: &[u8],
    ) -> Result<(HpkeRequest, Vec<u8>), String> {
        let public: Result<api_types::Blob, ErrorCode> =
            crate::query(pic, canister, caller, "get_hpke_public_key", ())?;
        let public = match public {
            Ok(public) => public,
            Err(ErrorCode::PolicyUnavailable) => {
                let controller = pic
                    .get_controllers(canister)
                    .into_iter()
                    .next()
                    .ok_or("missing controller for test key rotation")?;
                let rotated: Result<api_types::Blob, ErrorCode> =
                    crate::update(pic, canister, controller, "rotate_hpke_key", ())?;
                rotated.map_err(|e| format!("rotate_hpke_key: {e:?}"))?
            }
            Err(error) => return Err(format!("get_hpke_public_key: {error:?}")),
        };
        let request_id = self.next_request_id(method, caller);
        let expires_at = now_ms(pic) + DEFAULT_TTL_MS;
        let aad = hpke_envelope::envelope_aad(
            &self.network,
            canister.as_slice(),
            method,
            caller.as_slice(),
            &request_id,
            expires_at,
        );
        let ciphertext =
            hpke_envelope::seal(public.as_ref(), ENVELOPE_INFO, &aad, plaintext, &request_id)
                .map_err(|e| format!("seal request: {e}"))?;
        let envelope = HpkeRequest {
            key_id: public,
            network: network_of(&self.network),
            canister,
            method: method.into(),
            request_id: request_id.to_vec().into(),
            expires_at,
            client_public_key: self.public.to_vec().into(),
            aad: aad.clone().into(),
            ciphertext: ciphertext.into(),
        };
        Ok((envelope, aad))
    }

    pub fn decode_encoded<R: CandidType + DeserializeOwned>(
        &self,
        response: Result<HpkeResponse, ErrorCode>,
        request: &HpkeRequest,
        aad: &[u8],
    ) -> Result<R, String> {
        let response = response.map_err(|e| format!("private_call {}: {e:?}", request.method))?;
        if response.request_id != request.request_id {
            return Err("private response request ID mismatch".into());
        }
        let plaintext = self.open(aad, response.ciphertext.as_ref())?;
        candid::decode_one(&plaintext).map_err(|e| format!("decode private response: {e}"))
    }
    /// 決定的なクライアント鍵を持つクライアントを作る。
    pub fn new(seed: u8, network: &str) -> Self {
        let (secret, public) = hpke_envelope::derive_keypair(&[seed; 32]);
        Self {
            secret: secret.try_into().expect("32-byte secret"),
            public: public.try_into().expect("32-byte public"),
            network: network.to_string(),
        }
    }

    /// サーバの現行公開鍵（未生成はpanic。封筒を使う試験は鍵を生成しておく）。
    pub fn server_public(&self, pic: &PocketIc, canister: Principal, caller: Principal) -> Vec<u8> {
        let key: Result<Vec<u8>, ErrorCode> =
            crate::query(pic, canister, caller, "get_hpke_public_key", ()).expect("call");
        key.expect("get_hpke_public_key")
    }

    /// 封筒付きの呼び出し（要求IDは自動採番）。応答を復号して返す。
    pub fn call<Q, R>(
        &self,
        pic: &PocketIc,
        canister: Principal,
        caller: Principal,
        method: &str,
        query: &Q,
    ) -> Result<Result<R, ErrorCode>, String>
    where
        Q: CandidType,
        R: CandidType + DeserializeOwned,
    {
        let request_id = self.next_request_id(method, caller);
        self.call_with_request_id(pic, canister, caller, method, query, request_id)
    }

    /// 要求IDを指定した呼び出し（再送の試験に使う）。
    pub fn call_with_request_id<Q, R>(
        &self,
        pic: &PocketIc,
        canister: Principal,
        caller: Principal,
        method: &str,
        query: &Q,
        request_id: [u8; 32],
    ) -> Result<Result<R, ErrorCode>, String>
    where
        Q: CandidType,
        R: CandidType + DeserializeOwned,
    {
        let expires_at = now_ms(pic) + DEFAULT_TTL_MS;
        let (request, aad) =
            self.build_request(pic, canister, caller, method, query, request_id, expires_at)?;
        self.call_request(pic, canister, caller, method, &request, &aad)
    }

    /// 組み立て済みの封筒で呼び出し、応答を復号する（改竄の試験にも使う）。
    pub fn call_request<R: CandidType + DeserializeOwned>(
        &self,
        pic: &PocketIc,
        canister: Principal,
        caller: Principal,
        method: &str,
        request: &HpkeRequest,
        aad: &[u8],
    ) -> Result<Result<R, ErrorCode>, String> {
        let response: Result<HpkeResponse, ErrorCode> =
            crate::update(pic, canister, caller, method, request.clone()).expect("call");
        match response {
            Err(error) => Ok(Err(error)),
            Ok(response) => {
                assert_eq!(
                    response.request_id.as_ref(),
                    request.request_id.as_ref(),
                    "応答は要求IDを返す"
                );
                assert_ne!(response.ciphertext.as_ref(), aad, "応答は平文ではない");
                let plaintext = hpke_envelope::open(
                    &self.secret,
                    ENVELOPE_INFO,
                    aad,
                    response.ciphertext.as_ref(),
                )
                .map_err(|error| format!("open response: {error}"))?;
                candid::decode_one(&plaintext)
                    .map(Ok)
                    .map_err(|error| format!("decode response: {error}"))
            }
        }
    }

    /// 封筒を組み立てる。戻り値は封筒と、復号に使う`aad`。
    #[allow(clippy::too_many_arguments)]
    pub fn build_request<Q: CandidType>(
        &self,
        pic: &PocketIc,
        canister: Principal,
        caller: Principal,
        method: &str,
        query: &Q,
        request_id: [u8; 32],
        expires_at: u64,
    ) -> Result<(HpkeRequest, Vec<u8>), String> {
        let server_public = self.server_public(pic, canister, caller);
        let aad = hpke_envelope::envelope_aad(
            &self.network,
            canister.as_slice(),
            method,
            caller.as_slice(),
            &request_id,
            expires_at,
        );
        let plaintext = candid::encode_one(query).map_err(|error| error.to_string())?;
        let ciphertext =
            hpke_envelope::seal(&server_public, ENVELOPE_INFO, &aad, &plaintext, &request_id)
                .map_err(|error| format!("seal request: {error}"))?;
        let request = HpkeRequest {
            key_id: server_public.into(),
            network: network_of(&self.network),
            canister,
            method: method.to_string(),
            request_id: request_id.to_vec().into(),
            expires_at,
            client_public_key: self.public.to_vec().into(),
            aad: aad.clone().into(),
            ciphertext: ciphertext.into(),
        };
        Ok((request, aad))
    }

    /// クライアントが復号できるか（応答の平文をそのまま取り出す）。
    pub fn open(&self, aad: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        hpke_envelope::open(&self.secret, ENVELOPE_INFO, aad, ciphertext)
    }

    /// 要求IDを採番する（プロセス内で一意。再送の試験は`call_with_request_id`を使う）。
    pub fn next_request_id(&self, method: &str, caller: Principal) -> [u8; 32] {
        let seq = REQUEST_SEQ.fetch_add(1, Ordering::SeqCst);
        let mut input = Vec::new();
        input.extend_from_slice(method.as_bytes());
        input.extend_from_slice(&seq.to_be_bytes());
        input.extend_from_slice(caller.as_slice());
        hl_sign::keccak256(&input)
    }
}

/// 設定値のnetwork名からCandidの`Network`へ。
pub fn network_of(name: &str) -> Network {
    match name {
        "mainnet" => Network::Mainnet,
        "testnet" => Network::Testnet,
        _ => Network::Local,
    }
}

/// 試験で封筒を使うときのクライアント鍵の種（固定。秘密ではない）。
const TEST_CLIENT_SEED: u8 = 0xEE;

/// 要求IDの採番（プロセス内で単調増加）。
static REQUEST_SEQ: AtomicU64 = AtomicU64::new(0);

/// `get_account_snapshot` を封筒で呼ぶ。
pub fn get_account_snapshot(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    session: &api_types::auth::SessionHandle,
) -> Result<Result<api_types::order::AccountSnapshot, ErrorCode>, String> {
    client(TEST_CLIENT_SEED).call(
        pic,
        canister,
        caller,
        "get_account_snapshot",
        &api_types::envelope::SnapshotQuery {
            session: session.clone(),
        },
    )
}

/// `list_orders` を封筒で呼ぶ。
pub fn list_orders(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    session: &api_types::auth::SessionHandle,
    cursor: Option<api_types::Blob>,
    limit: u32,
) -> Result<Result<api_types::Paged<api_types::order::OrderSummary>, ErrorCode>, String> {
    client(TEST_CLIENT_SEED).call(
        pic,
        canister,
        caller,
        "list_orders",
        &api_types::envelope::ListQuery {
            session: session.clone(),
            cursor,
            limit,
        },
    )
}

/// `list_fills` を封筒で呼ぶ。
pub fn list_fills(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    session: &api_types::auth::SessionHandle,
    cursor: Option<api_types::Blob>,
    limit: u32,
) -> Result<Result<api_types::Paged<api_types::order::FillView>, ErrorCode>, String> {
    client(TEST_CLIENT_SEED).call(
        pic,
        canister,
        caller,
        "list_fills",
        &api_types::envelope::ListQuery {
            session: session.clone(),
            cursor,
            limit,
        },
    )
}

/// `cancel_order` を封筒で呼ぶ。
pub fn cancel_order(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    session: &api_types::auth::SessionHandle,
    order_id: api_types::Blob,
) -> Result<Result<(), ErrorCode>, String> {
    client(TEST_CLIENT_SEED).call(
        pic,
        canister,
        caller,
        "cancel_order",
        &api_types::envelope::CancelOrderQuery {
            session: session.clone(),
            order_id,
        },
    )
}
