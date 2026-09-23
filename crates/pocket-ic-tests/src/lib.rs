//! PocketICによるCanister統合・障害注入試験の共通ヘルパ。
//!
//! 実行には `POCKET_IC_BIN`（PocketICサーババイナリ）と、ビルド済みのCanister wasm
//! （`target/wasm32-unknown-unknown/release/*.wasm`）が必要である。
//! `bash scripts/pocket-ic-test.sh` が両方を用意して実行する。
//!
//! 本クレートはテスト専用であり、Canisterへは含めない。

pub mod envelope;
pub mod fixed_rng;

/// 既に作成済みの実取引口座IDを取得する。口座の作成や観測は行わない。
pub fn trading_account_id(
    pic: &pocket_ic::PocketIc,
    vault: candid::Principal,
    caller: candid::Principal,
    session: &api_types::auth::SessionHandle,
) -> api_types::Blob {
    let result: Result<Option<api_types::Blob>, api_types::error::ErrorCode> =
        update(pic, vault, caller, "get_trading_account", session.clone()).expect("call");
    let id = result
        .expect("get_trading_account")
        .expect("trading account exists");
    assert_eq!(id.len(), 32, "account ID is not an HL address");
    id
}

/// 建玉なしの取引口座を明示的に観測する。送信ヘルパからは呼ばない。
pub fn observe_empty_account(
    pic: &pocket_ic::PocketIc,
    core: candid::Principal,
    caller: candid::Principal,
    session: &api_types::auth::SessionHandle,
) {
    let result: Result<u32, api_types::error::ErrorCode> = update_args(
        pic,
        core,
        caller,
        "test_ingest_positions",
        (
            session.clone(),
            r#"{"assetPositions":[],"marginSummary":{"totalMarginUsed":"0"}}"#.to_string(),
        ),
    )
    .expect("call");
    assert_eq!(result.expect("observe empty account"), 0);
}

use candid::{CandidType, Principal};
use pocket_ic::common::rest::{
    CanisterHttpHeader, CanisterHttpMethod, CanisterHttpReject, CanisterHttpReplication,
    CanisterHttpReply, CanisterHttpResponse, MockCanisterHttpResponse,
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
///
/// 既定は `target/wasm32-unknown-unknown/release`。`scripts/pocket-ic-test.sh` は
/// test-venue付きのwasmを別ディレクトリへビルドし、`POCKET_IC_WASM_DIR` で指定する
/// （本番成果物と同じパスへ試験用ビルドを書かないため）。
pub fn wasm_path(file: &str) -> PathBuf {
    let root = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR")
        .rsplit_once("/crates/")
        .expect("crates directory")
        .0
        .to_string();
    let wasm_dir = std::env::var("POCKET_IC_WASM_DIR")
        .unwrap_or_else(|_| "target/wasm32-unknown-unknown/release".to_string());
    PathBuf::from(root).join(wasm_dir).join(file)
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

/// 注文を受け付けるための最小構成の `policy_registry` を用意する。
///
/// `trading_core` は停止状態とallowlistを policy へ照会し、未設定・照会失敗は
/// fail-closed（`PolicyUnavailable`）で拒否する。注文を扱う試験はこれを呼んで
/// allowlistと停止解除を設定する。戻り値は policy canister の principal。
pub fn configure_policy(
    pic: &PocketIc,
    core: Principal,
    controller: Principal,
    markets: &[&str],
) -> Principal {
    let policy = deploy(
        pic,
        POLICY_WASM,
        Some(vec![controller]),
        candid::encode_one(()).expect("encode ()"),
    );
    for method in ["set_operator", "set_sns_principal", "set_guard_principal"] {
        let result: Result<(), api_types::error::ErrorCode> =
            update(pic, policy, controller, method, controller).expect("call");
        result.unwrap_or_else(|error| panic!("{method}: {error:?}"));
    }
    let result: Result<(), api_types::error::ErrorCode> = update_args(
        pic,
        policy,
        controller,
        "set_policy_version",
        (
            1u64,
            markets
                .iter()
                .map(|m| m.to_string())
                .collect::<Vec<String>>(),
        ),
    )
    .expect("call");
    result.expect("set_policy_version");

    // 政策行が無い間は停止扱い（fail-closed）のため、SNS経路で解除する。
    let result: Result<(), api_types::error::ErrorCode> =
        update(pic, policy, controller, "clear_emergency_stop", ()).expect("call");
    result.expect("clear_emergency_stop");

    let result: Result<(), api_types::error::ErrorCode> =
        update(pic, core, controller, "set_policy_principal", policy).expect("call");
    result.expect("set_policy_principal");
    policy
}

/// 封筒を使う試験の前提：controllerがHPKE鍵を生成する（`api-contract.md` 6節）。
///
/// 鍵が無いと個人API（`get_account_snapshot`・`list_orders`・`list_fills`・
/// `cancel_order`）はfail-closedで拒否する。
pub fn rotate_hpke_key(pic: &PocketIc, canister: Principal, controller: Principal) -> Vec<u8> {
    let key: Result<Vec<u8>, api_types::error::ErrorCode> =
        update(pic, canister, controller, "rotate_hpke_key", ()).expect("call");
    key.expect("rotate_hpke_key")
}

/// 取引口座を用意して着金させ、取引可能なequityを作る。
///
/// `trading_core` は注文の受付時に取引口座のequityに対するリスク上限を検査するため、
/// 注文を扱う試験は事前にこれを呼ぶ。配分の着金を待たずに取引口座へ直接入金する
/// （`credit_venue_deposit`はcontrollerのみ）。戻り値は取引口座アドレス。
#[allow(clippy::too_many_arguments)]
pub fn fund_trading_account(
    pic: &PocketIc,
    vault: Principal,
    controller: Principal,
    caller: Principal,
    session: &api_types::auth::SessionHandle,
    request_id: &[u8],
    amount: u64,
    seed: u8,
) -> [u8; 20] {
    // 1. 本人へ入金を計上する（test-venue）。配分の予約が残る分を見込んで2倍入れる。
    let credit: Result<(), api_types::error::ErrorCode> = update_args(
        pic,
        vault,
        caller,
        "test_credit_deposit",
        (
            session.clone(),
            amount.saturating_mul(2),
            api_types::Blob::from(vec![seed; 32]),
        ),
    )
    .expect("call");
    credit.expect("credit");

    // 2. 配分の受付で取引口座が作られる（送信はしない）。
    let allocated: Result<api_types::fund::FundRequestAccepted, api_types::error::ErrorCode> =
        update(
            pic,
            vault,
            caller,
            "request_allocation",
            api_types::fund::AllocationRequest {
                session: session.clone(),
                client_request_id: api_types::Blob::from(request_id.to_vec()),
                amount,
                target: api_types::AccountKind::Trading,
                intent_signature: None,
            },
        )
        .expect("call");
    allocated.expect("allocation");

    // 3. 取引口座アドレスを取得し、着金させる。
    let trading: Result<api_types::Blob, api_types::error::ErrorCode> =
        update(pic, vault, caller, "get_trading_address", session.clone()).expect("call");
    let trading: [u8; 20] = trading
        .expect("trading address")
        .as_ref()
        .try_into()
        .expect("20-byte address");

    let credited: Result<bool, api_types::error::ErrorCode> = update_args(
        pic,
        vault,
        controller,
        "credit_venue_deposit",
        (
            api_types::Blob::from(vec![seed; 32]),
            amount,
            api_types::Blob::from(trading.to_vec()),
            "usdc".to_string(),
        ),
    )
    .expect("call");
    assert!(credited.expect("arrival"), "取引口座への着金を取り込む");
    trading
}

/// vaultでAgent世代を承認する（取引所へのoutcallはmockで受理させる）。
///
/// `trading_core` はvaultが承認した世代でしか署名しないため、注文を送信する試験は
/// 事前にこれを呼ぶ。
pub fn approve_agent_at_vault(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    session: &api_types::auth::SessionHandle,
    generation: u64,
    agent_address: &[u8],
) -> Result<api_types::fund::AgentGeneration, api_types::error::ErrorCode> {
    call_with_mocked_outcall(
        pic,
        vault,
        caller,
        "approve_agent_generation",
        (
            session.clone(),
            generation,
            api_types::Blob::from(agent_address.to_vec()),
        ),
        Ok((
            200,
            br#"{"status":"ok","response":{"type":"default"}}"#.to_vec(),
        )),
    )
    .expect("call")
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

/// mockしたoutcallの送信内容（URL・メソッド・本文・replication）。
///
/// 「正しい送信先・正しい本文・非replicatedで送った」ことを試験で検証するために使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedHttpCall {
    pub url: String,
    pub method: CanisterHttpMethod,
    pub body: Vec<u8>,
    pub replication: CanisterHttpReplication,
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
    call_with_mocked_outcall_captured(pic, canister, caller, method, arg, reply)
        .map(|(value, _call)| value)
}

/// `call_with_mocked_outcall` の捕捉版。mockしたoutcallの送信内容も返す。
///
/// outcallが出ずに呼び出しが終わった場合（送信前の失敗など）は捕捉が `None` になる。
pub fn call_with_mocked_outcall_captured<A, R>(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    method: &str,
    arg: A,
    reply: Result<(u16, Vec<u8>), (u64, String)>,
) -> Result<(R, Option<CapturedHttpCall>), String>
where
    A: candid::utils::ArgumentEncoder,
    R: CandidType + DeserializeOwned,
{
    let payload = candid::encode_args(arg).map_err(|error| format!("encode {method}: {error}"))?;
    let message_id = pic
        .submit_call(canister, caller, method, payload)
        .map_err(|error| format!("submit {method}: {error:?}"))?;

    let mut captured = None;
    for _ in 0..50 {
        pic.tick();
        let pending = pic.get_canister_http();
        if let Some(request) = pending.first() {
            captured = Some(CapturedHttpCall {
                url: request.url.clone(),
                method: request.http_method.clone(),
                body: request.body.clone(),
                replication: request.replication.clone(),
            });
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
            break;
        }
    }
    let Some(captured) = captured else {
        // outcallが出ないまま呼び出しが終わっている場合は、その応答を返す
        // （sweepが送信前に失敗した場合など、原因をテスト側で観測できるようにする）。
        if let Some(status) = pic.ingress_status(message_id) {
            let bytes = status.map_err(|error| format!("reject {method}: {error:?}"))?;
            let value: R =
                candid::decode_one(&bytes).map_err(|error| format!("decode {method}: {error}"))?;
            return Ok((value, None));
        }
        return Err(format!("{method}: no pending outcall to mock"));
    };

    let bytes = pic
        .await_call(message_id)
        .map_err(|error| format!("reject {method}: {error:?}"))?;
    let value: R =
        candid::decode_one(&bytes).map_err(|error| format!("decode {method}: {error}"))?;
    Ok((value, Some(captured)))
}

/// outcallを伴うupdate呼び出しを、**送信内容で振り分けて**複数のoutcallをmockし完了させる。
///
/// sweep（送信＋照合）のように1回の呼び出しで複数のoutcallが出る経路を試験する。
/// `route` は観測した送信内容から応答を決める（URL・本文の種別で振り分ける）。
pub fn call_with_routed_outcalls<A, R, F>(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    method: &str,
    arg: A,
    route: F,
) -> Result<(R, Vec<CapturedHttpCall>), String>
where
    A: candid::utils::ArgumentEncoder,
    R: CandidType + DeserializeOwned,
    F: Fn(&CapturedHttpCall) -> Result<(u16, Vec<u8>), (u64, String)>,
{
    let payload = candid::encode_args(arg).map_err(|error| format!("encode {method}: {error}"))?;
    let message_id = pic
        .submit_call(canister, caller, method, payload)
        .map_err(|error| format!("submit {method}: {error:?}"))?;

    let mut captured: Vec<CapturedHttpCall> = Vec::new();
    let mut completed = false;
    for _ in 0..200 {
        pic.tick();
        for request in pic.get_canister_http() {
            let call = CapturedHttpCall {
                url: request.url.clone(),
                method: request.http_method.clone(),
                body: request.body.clone(),
                replication: request.replication.clone(),
            };
            let reply = route(&call);
            captured.push(call);
            let response = match reply {
                Ok((status, body)) => CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
                    status,
                    headers: vec![CanisterHttpHeader {
                        name: "Content-Type".to_string(),
                        value: "application/json".to_string(),
                    }],
                    body,
                }),
                Err((code, message)) => {
                    CanisterHttpResponse::CanisterHttpReject(CanisterHttpReject {
                        reject_code: code,
                        message,
                    })
                }
            };
            pic.mock_canister_http_response(MockCanisterHttpResponse {
                subnet_id: request.subnet_id,
                request_id: request.request_id,
                response,
                additional_responses: Vec::new(),
            });
        }
        if pic.ingress_status(message_id.clone()).is_some() {
            completed = true;
            break;
        }
    }
    if !completed {
        return Err(format!("{method}: the call did not complete"));
    }
    let bytes = pic
        .await_call(message_id)
        .map_err(|error| format!("reject {method}: {error:?}"))?;
    let value: R =
        candid::decode_one(&bytes).map_err(|error| format!("decode {method}: {error}"))?;
    Ok((value, captured))
}

/// Hyperliquidの`/info`（建玉なし）への既定応答。
pub const EMPTY_POSITIONS: &[u8] = br#"{"assetPositions":[]}"#;
/// Hyperliquidの`/info`（約定なし）への既定応答。
pub const NO_FILLS: &[u8] = b"[]";

fn is_update_leverage(call: &CapturedHttpCall) -> bool {
    serde_json::from_slice::<serde_json::Value>(&call.body)
        .ok()
        .and_then(|body| body.get("action")?.get("type")?.as_str().map(str::to_owned))
        .is_some_and(|kind| kind == "updateLeverage")
}

fn leverage_accepted() -> Vec<u8> {
    br#"{"status":"ok","response":{"type":"default"}}"#.to_vec()
}

/// HLの`/exchange`と`/info`へ、用途別の応答を返すルータ。
///
/// `sweep`は1回の呼び出しで送信と照合の複数のoutcallを出すため、送信内容
/// （URLと本文の`type`）で応答を振り分ける。
pub fn venue_router(
    exchange: &[u8],
    positions: &[u8],
    fills: &[u8],
    status: &[u8],
) -> impl Fn(&CapturedHttpCall) -> Result<(u16, Vec<u8>), (u64, String)> {
    let exchange = exchange.to_vec();
    let positions = positions.to_vec();
    let fills = fills.to_vec();
    let status = status.to_vec();
    move |call| {
        if call.url.contains("/exchange") {
            if is_update_leverage(call) {
                return Ok((200, leverage_accepted()));
            }
            return Ok((200, exchange.clone()));
        }
        let query: serde_json::Value = serde_json::from_slice(&call.body).unwrap_or_default();
        match query.get("type").and_then(|value| value.as_str()) {
            Some("clearinghouseState") => Ok((200, positions.clone())),
            Some("userFills") => Ok((200, fills.clone())),
            Some("orderStatus") => Ok((200, status.clone())),
            other => Err((1, format!("unexpected info query: {other:?}"))),
        }
    }
}

/// 送信応答だけを指定し、照合は「建玉なし・約定なし・問い合わせたoidはopen」を返すルータ。
pub fn venue_router_default(
    exchange: &[u8],
) -> impl Fn(&CapturedHttpCall) -> Result<(u16, Vec<u8>), (u64, String)> {
    let exchange = exchange.to_vec();
    move |call| {
        if call.url.contains("/exchange") {
            if is_update_leverage(call) {
                return Ok((200, leverage_accepted()));
            }
            return Ok((200, exchange.clone()));
        }
        let query: serde_json::Value = serde_json::from_slice(&call.body).unwrap_or_default();
        match query.get("type").and_then(|value| value.as_str()) {
            Some("clearinghouseState") => Ok((200, EMPTY_POSITIONS.to_vec())),
            Some("userFills") => Ok((200, NO_FILLS.to_vec())),
            Some("orderStatus") => {
                let oid = query.get("oid").cloned().unwrap_or(serde_json::Value::Null);
                let body = serde_json::json!({ "status": "open", "order": { "oid": oid } });
                Ok((200, body.to_string().into_bytes()))
            }
            other => Err((1, format!("unexpected info query: {other:?}"))),
        }
    }
}

/// `sweep`（`test_sweep_now`）を、送信応答を指定して実行する。
///
/// 照合のoutcallには既定応答を返す。送信だけを検証する試験で使う。
pub fn sweep_with_venue_outcalls<R>(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    exchange: Vec<u8>,
) -> Result<R, String>
where
    R: CandidType + DeserializeOwned,
{
    call_with_routed_outcalls::<(), R, _>(
        pic,
        canister,
        caller,
        "test_sweep_now",
        (),
        venue_router_default(&exchange),
    )
    .map(|(value, _calls)| value)
}

/// `sweep`を、送信のoutcallが失敗する状況（応答喪失）で実行する。
///
/// 照合のoutcallには既定応答を返す。結果不明の分類を検証する試験で使う。
pub fn sweep_with_failed_send<R>(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
) -> Result<R, String>
where
    R: CandidType + DeserializeOwned,
{
    call_with_routed_outcalls::<(), R, _>(pic, canister, caller, "test_sweep_now", (), |call| {
        if call.url.contains("/exchange") {
            Err((3, "outcall failed".to_string()))
        } else {
            venue_router_default(b"")(call)
        }
    })
    .map(|(value, _calls)| value)
}

/// Canisterをアップグレードする（`post_upgrade`の検証に使う）。
pub fn upgrade(pic: &PocketIc, canister: Principal, file: &str, init_arg: Vec<u8>) {
    pic.upgrade_canister(canister, wasm(file), init_arg, None)
        .unwrap_or_else(|error| panic!("upgrade {file}: {error:?}"));
}
