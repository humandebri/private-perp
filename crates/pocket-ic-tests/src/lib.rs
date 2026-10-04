//! PocketICによるCanister統合・障害注入試験の共通ヘルパ。
//!
//! 実行には `POCKET_IC_BIN`（PocketICサーババイナリ）と、ビルド済みのCanister wasm
//! （`target/wasm32-unknown-unknown/release/*.wasm`）が必要である。
//! `bash scripts/pocket-ic-test.sh` が両方を用意して実行する。
//!
//! 本クレートはテスト専用であり、Canisterへは含めない。

pub mod envelope;
pub mod fixed_rng;

/// Configure a real signed local eligibility token for a test session.
/// The issuer secret is generated from the host OS per test process and never
/// committed, included in frontend code, or stored in a canister.
pub fn activate_local_user(
    pic: &pocket_ic::PocketIc,
    vault: candid::Principal,
    caller: candid::Principal,
    session: &api_types::auth::SessionHandle,
) {
    use api_types::eligibility::{EligibilityClaims, EligibilityStatus, EligibilityToken};
    use api_types::error::ErrorCode;
    use std::io::Read;
    thread_local! {
        static ISSUERS: std::cell::RefCell<std::collections::HashMap<candid::Principal, [u8; 32]>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let controller = pic
        .get_controllers(vault)
        .into_iter()
        .next()
        .expect("vault controller");
    let sns = principal(239);
    let configured: Result<Option<(u64, api_types::Blob)>, ErrorCode> =
        query(pic, vault, caller, "get_eligibility_configuration", ()).expect("config query");
    let key = if configured.expect("eligibility config").is_none() {
        let guard = deploy(
            pic,
            CONTROL_GUARD_WASM,
            Some(vec![controller]),
            candid::encode_one(()).unwrap(),
        );
        let configured: Result<(), ErrorCode> =
            update(pic, guard, controller, "set_sns_principal", sns).unwrap();
        configured.unwrap();
        let configured: Result<(), ErrorCode> =
            update(pic, vault, controller, "set_journal_guard", guard).unwrap();
        configured.unwrap();
        let mut key = [0u8; 32];
        loop {
            std::fs::File::open("/dev/urandom")
                .unwrap()
                .read_exact(&mut key)
                .unwrap();
            if hl_sign::address_from_secret(&key).is_ok() {
                break;
            }
        }
        let address = hl_sign::address_from_secret(&key).unwrap();
        let configured: Result<(), ErrorCode> = update_args(
            pic,
            guard,
            sns,
            "configure_eligibility",
            (vault, 1u64, address.to_vec(), true),
        )
        .unwrap();
        configured.unwrap();
        let configured: Result<(), ErrorCode> = update_args(
            pic,
            guard,
            sns,
            "configure_cycles",
            (vault, 100_000_000_000u128, 1_000_000_000_000u128),
        )
        .unwrap();
        configured.unwrap();
        ISSUERS.with(|issuers| {
            issuers.borrow_mut().insert(vault, key);
        });
        key
    } else {
        ISSUERS.with(|issuers| {
            *issuers
                .borrow()
                .get(&vault)
                .expect("issuer key for existing vault")
        })
    };
    let client = envelope::client(0xD3);
    let prepared: Result<api_types::Blob, ErrorCode> = client
        .call_encoded(
            pic,
            vault,
            caller,
            "prepare_trading_account",
            &candid::encode_one(session.clone()).unwrap(),
        )
        .unwrap();
    let account = prepared.unwrap();
    let expires_at = envelope::now_ms(pic) + 60 * 60 * 1000;
    let claims: Result<EligibilityClaims, ErrorCode> = client
        .call_encoded(
            pic,
            vault,
            caller,
            "eligibility_signing_claims",
            &candid::encode_args((session.clone(), expires_at)).unwrap(),
        )
        .unwrap();
    let claims = claims.unwrap();
    assert_eq!(claims.account_id, account);
    let encoded = candid::encode_one(&claims).unwrap();
    let digest = hl_sign::keccak256_concat(&[b"private-perp/eligibility/v1", &encoded]);
    let signature = hl_sign::sign_digest_for_tests(&digest, &key).unwrap();
    let token = EligibilityToken {
        claims,
        signature: signature.to_bytes65().to_vec().into(),
    };
    let registered: Result<EligibilityStatus, ErrorCode> = client
        .call_encoded(
            pic,
            vault,
            caller,
            "register_eligibility",
            &candid::encode_args((session.clone(), token)).unwrap(),
        )
        .unwrap();
    assert!(registered.unwrap().eligible);
}

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
pub const SEND_JOURNAL_WASM: &str = "send_journal.wasm";

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
    if file == FUNDS_VAULT_WASM || file == TRADING_CORE_WASM {
        let controller = pic
            .get_controllers(canister)
            .into_iter()
            .next()
            .expect("worker controller");
        let journal = deploy(
            pic,
            SEND_JOURNAL_WASM,
            Some(vec![controller]),
            candid::encode_one(()).expect("journal init"),
        );
        let role = if file == FUNDS_VAULT_WASM {
            "vault"
        } else {
            "core"
        };
        let registered: Result<(), api_types::error::ErrorCode> = update_args(
            pic,
            journal,
            controller,
            "register_worker",
            (role.to_string(), canister),
        )
        .expect("register journal worker call");
        registered.expect("register journal worker");
        let configured: Result<(), api_types::error::ErrorCode> =
            update(pic, canister, controller, "set_send_journal", journal)
                .expect("configure send journal call");
        configured.expect("configure send journal");
    }
    if file == FUNDS_VAULT_WASM {
        let vault_controller = pic
            .get_controllers(canister)
            .into_iter()
            .next()
            .expect("vault controller");
        configure_vault_budget(pic, canister, vault_controller);
    }
    canister
}

/// 引数なしCanisterをdeployする。
pub fn deploy_default(pic: &PocketIc, file: &str) -> Principal {
    deploy(pic, file, None, candid::encode_one(()).expect("encode ()"))
}

/// vault単体試験も本番と同じ予算取得を通す。coreとの結合試験では
/// `configure_policy`が両workerを同じpolicyへ差し替える。
fn configure_vault_budget(pic: &PocketIc, vault: Principal, vault_controller: Principal) {
    let controller = principal(240);
    let policy = deploy(
        pic,
        POLICY_WASM,
        Some(vec![controller]),
        candid::encode_one(()).expect("encode ()"),
    );
    let guard: Result<(), api_types::error::ErrorCode> =
        update(pic, policy, controller, "set_guard_principal", controller).expect("call");
    guard.expect("set_guard_principal");
    let sns: Result<(), api_types::error::ErrorCode> =
        update(pic, policy, controller, "set_sns_principal", controller).expect("call");
    sns.expect("set_sns_principal");
    let version: Result<(), api_types::error::ErrorCode> = update_args(
        pic,
        policy,
        controller,
        "set_policy_version",
        (1u64, vec!["BTC".to_string(), "ETH".to_string()]),
    )
    .expect("call");
    version.expect("set_policy_version");
    let active: Result<(), api_types::error::ErrorCode> =
        update(pic, policy, controller, "clear_emergency_stop", ()).expect("call");
    active.expect("clear_emergency_stop");
    let worker: Result<(), api_types::error::ErrorCode> = update_args(
        pic,
        policy,
        controller,
        "register_budget_worker",
        ("vault".to_string(), vault),
    )
    .expect("call");
    worker.expect("register_budget_worker");
    let budget: Result<(), api_types::error::ErrorCode> = update(
        pic,
        policy,
        controller,
        "configure_rest_budget",
        api_types::operations::RestBudgetConfig {
            capacity: 1200,
            exit_reserve: 300,
        },
    )
    .expect("call");
    budget.expect("configure_rest_budget");
    let configured: Result<(), api_types::error::ErrorCode> =
        update(pic, vault, vault_controller, "set_policy_principal", policy).expect("call");
    configured.expect("set_policy_principal");
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
    let result: Result<(), api_types::error::ErrorCode> = update_args(
        pic,
        policy,
        controller,
        "register_budget_worker",
        ("core".to_string(), core),
    )
    .expect("call");
    result.expect("register_budget_worker");
    let vault: Option<Principal> =
        query(pic, core, controller, "get_vault_principal", ()).expect("get_vault_principal");
    if let Some(vault) = vault {
        let result: Result<(), api_types::error::ErrorCode> =
            update(pic, vault, controller, "set_core_principal", core).expect("call");
        result.expect("set vault core principal");
        let result: Result<(), api_types::error::ErrorCode> = update_args(
            pic,
            policy,
            controller,
            "register_budget_worker",
            ("vault".to_string(), vault),
        )
        .expect("call");
        result.expect("register vault budget worker");
        let result: Result<(), api_types::error::ErrorCode> =
            update(pic, vault, controller, "set_policy_principal", policy).expect("call");
        result.expect("set vault policy principal");
    }
    let result: Result<(), api_types::error::ErrorCode> = update(
        pic,
        policy,
        controller,
        "configure_rest_budget",
        api_types::operations::RestBudgetConfig {
            capacity: 1200,
            exit_reserve: 300,
        },
    )
    .expect("call");
    result.expect("configure_rest_budget");
    configure_core_admission(pic, core, controller);
    policy
}

/// Configure cycles and BTC/ETH observations through the same guard and mock HL paths
/// as production admission. A test that uses a custom policy can call this directly.
pub fn configure_core_admission(pic: &PocketIc, core: Principal, controller: Principal) {
    let guard = deploy(
        pic,
        CONTROL_GUARD_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), api_types::error::ErrorCode> =
        update(pic, guard, controller, "set_sns_principal", controller).unwrap();
    set.unwrap();
    let set: Result<(), api_types::error::ErrorCode> =
        update(pic, core, controller, "set_journal_guard", guard).unwrap();
    set.unwrap();
    let set: Result<(), api_types::error::ErrorCode> = update_args(
        pic,
        guard,
        controller,
        "configure_cycles",
        (core, 100_000_000_000u128, 1_000_000_000_000u128),
    )
    .unwrap();
    set.unwrap();
    for (market, index, depth) in [("BTC", 2, 10_000), ("ETH", 1, 1_000)] {
        let set: Result<(), api_types::error::ErrorCode> = update_args(
            pic,
            guard,
            controller,
            "configure_market_threshold",
            (
                core,
                api_types::operations_status::MarketThreshold {
                    market: market.into(),
                    expected_index: index,
                    min_day_notional_usdc: 1_000_000,
                    max_spread_bps: 20,
                    min_each_side_depth_usdc: depth,
                },
            ),
        )
        .unwrap();
        set.unwrap();
    }
    let (refreshed, _): (Result<(), api_types::error::ErrorCode>, _) = call_with_routed_outcalls(
        pic, core, controller, "refresh_market", (), |call| {
            let query: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
            match query.get("type").and_then(|v| v.as_str()) {
                Some("metaAndAssetCtxs") => Ok((200, br#"[{"universe":[{"name":"SOL"},{"name":"ETH"},{"name":"BTC"}]},[{"dayNtlVlm":"10000000"},{"dayNtlVlm":"100000000"},{"dayNtlVlm":"500000000"}]]"#.to_vec())),
                Some("l2Book") => {
                    let mid = if query.get("coin").and_then(|v| v.as_str()) == Some("BTC") { 60000 } else { 3000 };
                    Ok((200, serde_json::json!({
                        "coin": query["coin"],
                        "time": pic.get_time().as_nanos_since_unix_epoch() / 1_000_000,
                        "levels": [[{"px":(mid - 1).to_string(),"sz":"1.25"}],
                            [{"px":(mid + 1).to_string(),"sz":"1.10"}]]
                    }).to_string().into_bytes()))
                }
                other => Err((1, format!("unexpected market query: {other:?}"))),
            }
        }).expect("market refresh outcalls");
    refreshed.expect("refresh market");
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
    if private_vault_method(method) {
        return envelope::client(0xED).call_encoded(pic, canister, caller, method, &payload);
    }
    if private_core_method(method) {
        let wrapped = candid::encode_one(api_types::Blob::from(payload))
            .map_err(|e| format!("encode core private payload: {e}"))?;
        return envelope::client(0xEC).call_encoded(pic, canister, caller, method, &wrapped);
    }
    let bytes = pic
        .update_call(canister, caller, method, payload)
        .map_err(|error| format!("reject {method}: {error:?}"))?;
    candid::decode_one(&bytes).map_err(|error| format!("decode {method}: {error}"))
}

fn private_vault_method(method: &str) -> bool {
    matches!(
        method,
        "revoke_session"
            | "approve_agent_generation"
            | "request_allocation"
            | "request_withdrawal"
            | "provision_reserve_account"
            | "prepare_trading_account"
            | "request_recovery"
    )
}

fn private_core_method(method: &str) -> bool {
    matches!(
        method,
        "submit_order" | "close_position" | "close_all" | "request_agent_generation" | "cancel_all"
    )
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
    if private_vault_method(method) {
        return envelope::client(0xED).call_encoded(pic, canister, caller, method, &payload);
    }
    if private_core_method(method) {
        let wrapped = candid::encode_one(api_types::Blob::from(payload))
            .map_err(|e| format!("encode core private payload: {e}"))?;
        return envelope::client(0xEC).call_encoded(pic, canister, caller, method, &wrapped);
    }
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
    let private = if private_vault_method(method) {
        let client = envelope::client(0xED);
        let (request, aad) = client.prepare_encoded(pic, canister, caller, method, &payload)?;
        Some((client, request, aad))
    } else {
        None
    };
    let encoded = match private.as_ref() {
        Some((_, request, _)) => {
            candid::encode_one(request).map_err(|e| format!("encode envelope: {e}"))?
        }
        None => payload,
    };
    let message_id = pic
        .submit_call(
            canister,
            caller,
            if private.is_some() {
                "private_call"
            } else {
                method
            },
            encoded,
        )
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
            let value: R = decode_captured_response(&bytes, method, private.as_ref())?;
            return Ok((value, None));
        }
        return Err(format!("{method}: no pending outcall to mock"));
    };

    let bytes = pic
        .await_call(message_id)
        .map_err(|error| format!("reject {method}: {error:?}"))?;
    let value: R = decode_captured_response(&bytes, method, private.as_ref())?;
    Ok((value, Some(captured)))
}

fn decode_captured_response<R: CandidType + DeserializeOwned>(
    bytes: &[u8],
    method: &str,
    private: Option<&(
        envelope::EnvelopeClient,
        api_types::envelope::HpkeRequest,
        Vec<u8>,
    )>,
) -> Result<R, String> {
    if let Some((client, request, aad)) = private {
        let response: Result<api_types::envelope::HpkeResponse, api_types::error::ErrorCode> =
            candid::decode_one(bytes).map_err(|e| format!("decode {method} envelope: {e}"))?;
        client.decode_encoded(response, request, aad)
    } else {
        candid::decode_one(bytes).map_err(|error| format!("decode {method}: {error}"))
    }
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
            Some("userFillsByTime") => Ok((200, fills.clone())),
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
            Some("userFillsByTime") => Ok((200, NO_FILLS.to_vec())),
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

/// Explicit user action in tests; never called by a sweep helper.
pub fn resume_manual_work(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    session: &api_types::auth::SessionHandle,
    core: bool,
    kind: &str,
) -> usize {
    let client = envelope::client(0xEA);
    let wrap = |payload: Vec<u8>| {
        if core {
            candid::encode_one(api_types::Blob::from(payload)).unwrap()
        } else {
            payload
        }
    };
    let work: Result<Vec<(String, Vec<u8>, u64)>, api_types::error::ErrorCode> = client
        .call_encoded(
            pic,
            canister,
            caller,
            "get_manual_work",
            &wrap(candid::encode_one(session).unwrap()),
        )
        .unwrap();
    let mut count = 0;
    for item in work.unwrap().into_iter().filter(|item| item.0 == kind) {
        let result: Result<(), api_types::error::ErrorCode> = client
            .call_encoded(
                pic,
                canister,
                caller,
                "resume_manual_work",
                &wrap(
                    candid::encode_args((
                        session.clone(),
                        item.0,
                        api_types::Blob::from(item.1),
                        item.2,
                    ))
                    .unwrap(),
                ),
            )
            .unwrap();
        result.unwrap();
        count += 1;
    }
    count
}

/// A rejected manual grant must retain the exact stopped row and its generation.
pub fn assert_manual_work_blocked(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    session: &api_types::auth::SessionHandle,
    core: bool,
    work: (&str, Option<&[u8]>),
    reason: &str,
) {
    let (kind, id) = work;
    let client = envelope::client(0xEB);
    let wrap = |payload: Vec<u8>| {
        if core {
            candid::encode_one(api_types::Blob::from(payload)).unwrap()
        } else {
            payload
        }
    };
    let list = || {
        let result: Result<Vec<(String, Vec<u8>, u64)>, api_types::error::ErrorCode> = client
            .call_encoded(
                pic,
                canister,
                caller,
                "get_manual_work",
                &wrap(candid::encode_one(session).unwrap()),
            )
            .unwrap();
        result.unwrap()
    };
    let before = list();
    let item = before
        .iter()
        .find(|item| item.0 == kind && id.is_none_or(|id| item.1 == id))
        .expect("stopped work");
    for _ in 0..2 {
        let result: Result<(), api_types::error::ErrorCode> = client
            .call_encoded(
                pic,
                canister,
                caller,
                "resume_manual_work",
                &wrap(
                    candid::encode_args((
                        session.clone(),
                        item.0.clone(),
                        api_types::Blob::from(item.1.clone()),
                        item.2,
                    ))
                    .unwrap(),
                ),
            )
            .unwrap();
        assert!(
            matches!(result, Err(api_types::error::ErrorCode::BadRequest { detail, .. }) if detail.contains(reason))
        );
        assert_eq!(
            list(),
            before,
            "denial must preserve stopped status and generation"
        );
    }
}
