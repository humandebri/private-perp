//! PocketICによるCanister統合・障害注入試験の共通ヘルパ。
//!
//! 実行には `POCKET_IC_BIN`（PocketICサーババイナリ）と、ビルド済みのCanister wasm
//! （`target/wasm32-unknown-unknown/release/*.wasm`）が必要である。
//! `bash scripts/pocket-ic-test.sh` が両方を用意して実行する。
//!
//! 本クレートはテスト専用であり、Canisterへは含めない。

use candid::{CandidType, Principal};
use pocket_ic::common::rest::{
    CanisterHttpHeader, CanisterHttpReject, CanisterHttpReply, CanisterHttpResponse,
    MockCanisterHttpResponse,
};
use pocket_ic::{CanisterSettings, PocketIc, PocketIcBuilder};
use serde::de::DeserializeOwned;
use std::path::PathBuf;

/// Canister wasmのファイル名（`crates/*` のlib名に対応）。
pub const POLICY_WASM: &str = "policy.wasm";
pub const FUNDS_VAULT_WASM: &str = "funds_vault.wasm";
pub const CONTROL_GUARD_WASM: &str = "control_guard.wasm";
pub const TRADING_CORE_WASM: &str = "trading_core.wasm";

/// Canisterへ供給するcycles（テスト用）。
pub const TEST_CYCLES: u128 = 10_000_000_000_000_000;

/// PocketICインスタンスを1つ作る。
///
/// アプリケーションサブネット（Canister用）に加えて**テスト用閾値鍵サブネット**を
/// 作る。これが無いと `ecdsa_public_key` / `sign_with_ecdsa` は
/// `existing keys: []` で拒否される（`PocketIc::new()` の既定トポロジ）。
pub fn pic() -> PocketIc {
    PocketIcBuilder::new()
        .with_application_subnet()
        .with_test_threshold_keys_subnet()
        .build()
}

/// テスト用のPrincipal（呼び出し主体の取り違え試験に使う）。
pub fn principal(seed: u8) -> Principal {
    let mut bytes = [0u8; 29];
    bytes[0] = seed;
    bytes[28] = 1;
    Principal::from_slice(&bytes)
}

/// ビルド済みwasmのパス。
pub fn wasm_path(file: &str) -> PathBuf {
    let root = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR")
        .rsplit_once("/crates/")
        .expect("crates directory")
        .0
        .to_string();
    PathBuf::from(root)
        .join("target/wasm32-unknown-unknown/release")
        .join(file)
}

/// wasmを読み込む。未ビルドの場合はスクリプトの実行を促す。
pub fn wasm(file: &str) -> Vec<u8> {
    let path = wasm_path(file);
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "{} を読めません（{error}）。`bash scripts/pocket-ic-test.sh` でwasmをビルドしてください",
            path.display()
        )
    })
}

/// Canisterを作成し、wasmをinstallする。
pub fn deploy(
    pic: &PocketIc,
    file: &str,
    controllers: Option<Vec<Principal>>,
    init_arg: Vec<u8>,
) -> Principal {
    let settings = controllers.clone().map(|controllers| CanisterSettings {
        controllers: Some(controllers),
        ..Default::default()
    });
    let canister = pic.create_canister_with_settings(None, settings);
    pic.add_cycles(canister, TEST_CYCLES);
    // 明示的なcontrollerを指定した場合、installはそのcontrollerとして行う
    // （既定のsenderはcontrollerではないため拒否される）。
    let sender = controllers
        .as_ref()
        .and_then(|controllers| controllers.first().copied());
    pic.install_canister(canister, wasm(file), init_arg, sender);
    canister
}

/// 引数なしCanisterをdeployする。
pub fn deploy_default(pic: &PocketIc, file: &str) -> Principal {
    deploy(pic, file, None, candid::encode_one(()).expect("encode ()"))
}

/// update呼び出しを行い、応答をデコードする。
pub fn update<A, R>(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    method: &str,
    arg: A,
) -> Result<R, String>
where
    A: CandidType,
    R: CandidType + DeserializeOwned,
{
    let payload = candid::encode_one(arg).map_err(|error| format!("encode {method}: {error}"))?;
    let bytes = pic
        .update_call(canister, caller, method, payload)
        .map_err(|error| format!("reject {method}: {error:?}"))?;
    candid::decode_one(&bytes).map_err(|error| format!("decode {method}: {error}"))
}

/// query呼び出しを行い、応答をデコードする。
pub fn query<A, R>(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    method: &str,
    arg: A,
) -> Result<R, String>
where
    A: CandidType,
    R: CandidType + DeserializeOwned,
{
    let payload = candid::encode_one(arg).map_err(|error| format!("encode {method}: {error}"))?;
    let bytes = pic
        .query_call(canister, caller, method, payload)
        .map_err(|error| format!("reject {method}: {error:?}"))?;
    candid::decode_one(&bytes).map_err(|error| format!("decode {method}: {error}"))
}

/// Canisterのログを取得する（内部状態遷移の観測に使う。平文intentは出さない）。
pub fn logs(pic: &PocketIc, canister: Principal, sender: Principal) -> String {
    let records = pic
        .fetch_canister_logs(canister, sender)
        .expect("fetch canister logs");
    records
        .into_iter()
        .map(|record| String::from_utf8_lossy(&record.content).to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 複数引数のupdate呼び出し（Candidの複数引数として符号化する）。
pub fn update_args<A, R>(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    method: &str,
    arg: A,
) -> Result<R, String>
where
    A: candid::utils::ArgumentEncoder,
    R: CandidType + DeserializeOwned,
{
    let payload = candid::encode_args(arg).map_err(|error| format!("encode {method}: {error}"))?;
    let bytes = pic
        .update_call(canister, caller, method, payload)
        .map_err(|error| format!("reject {method}: {error:?}"))?;
    candid::decode_one(&bytes).map_err(|error| format!("decode {method}: {error}"))
}

/// 複数引数のquery呼び出し。
pub fn query_args<A, R>(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    method: &str,
    arg: A,
) -> Result<R, String>
where
    A: candid::utils::ArgumentEncoder,
    R: CandidType + DeserializeOwned,
{
    let payload = candid::encode_args(arg).map_err(|error| format!("encode {method}: {error}"))?;
    let bytes = pic
        .query_call(canister, caller, method, payload)
        .map_err(|error| format!("reject {method}: {error:?}"))?;
    candid::decode_one(&bytes).map_err(|error| format!("decode {method}: {error}"))
}

/// outcallを伴うupdate呼び出しを、応答をmockして完了させる。
///
/// PocketICのoutcall mockは `subnet_id` + `request_id` 指定であり、URL一致ではない。
/// `submit_call` → tick → `get_canister_http()` → mock → `await_call` の順で駆動する。
pub fn call_with_mocked_outcall<A, R>(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    method: &str,
    arg: A,
    reply: Result<(u16, Vec<u8>), (u64, String)>,
) -> Result<R, String>
where
    A: candid::utils::ArgumentEncoder,
    R: CandidType + DeserializeOwned,
{
    let payload = candid::encode_args(arg).map_err(|error| format!("encode {method}: {error}"))?;
    let message_id = pic
        .submit_call(canister, caller, method, payload)
        .map_err(|error| format!("submit {method}: {error:?}"))?;

    let mut mocked = false;
    for _ in 0..50 {
        pic.tick();
        let pending = pic.get_canister_http();
        if let Some(request) = pending.first() {
            let response = match &reply {
                Ok((status, body)) => CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
                    status: *status,
                    headers: vec![CanisterHttpHeader {
                        name: "Content-Type".to_string(),
                        value: "application/json".to_string(),
                    }],
                    body: body.clone(),
                }),
                Err((code, message)) => {
                    CanisterHttpResponse::CanisterHttpReject(CanisterHttpReject {
                        reject_code: *code,
                        message: message.clone(),
                    })
                }
            };
            pic.mock_canister_http_response(MockCanisterHttpResponse {
                subnet_id: request.subnet_id,
                request_id: request.request_id,
                response,
                additional_responses: Vec::new(),
            });
            mocked = true;
            break;
        }
    }
    if !mocked {
        // outcallが出ないまま呼び出しが終わっている場合は、その応答を返す
        // （sweepが送信前に失敗した場合など、原因をテスト側で観測できるようにする）。
        if let Some(status) = pic.ingress_status(message_id) {
            let bytes = status.map_err(|error| format!("reject {method}: {error:?}"))?;
            return candid::decode_one(&bytes).map_err(|error| format!("decode {method}: {error}"));
        }
        return Err(format!("{method}: no pending outcall to mock"));
    }

    let bytes = pic
        .await_call(message_id)
        .map_err(|error| format!("reject {method}: {error:?}"))?;
    candid::decode_one(&bytes).map_err(|error| format!("decode {method}: {error}"))
}
