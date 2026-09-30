//! Fixed-seed mixed workflow against the real vault/core pipelines and mock HL.

use api_types::auth::{ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest};
use api_types::error::ErrorCode;
use api_types::fund::{AgentGeneration, FundRequestAccepted, FundStatus};
use api_types::operations::RestBudgetStatus;
use api_types::operations_status::MarketStatus;
use api_types::order::{OrderKind, Side, SubmitOrderArgs, SubmitOrderResult, SweepOutcome};
use api_types::{Blob, Network};
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, approve_agent_at_vault, call_with_routed_outcalls,
    configure_policy, deploy, envelope, fund_trading_account, observe_empty_account, pic,
    principal, query, rotate_hpke_key, trading_account_id, update, update_args,
};
use std::time::Instant;

const ACCEPTED: &[u8] = br#"{"status":"ok","response":{"type":"default"}}"#;
const EMPTY_STATE: &[u8] = br#"{"assetPositions":[],"marginSummary":{"accountValue":"10000","totalMarginUsed":"0"},"withdrawable":"10000"}"#;
const UNIVERSE: &str = r#"[{"name":"SOL","szDecimals":0},{"name":"ETH","szDecimals":5},{"name":"BTC","szDecimals":5}]"#;

fn session(
    pic: &pocket_ic::PocketIc,
    vault: candid::Principal,
    caller: candid::Principal,
    seed: u8,
) -> api_types::auth::SessionHandle {
    let mut key = [0; 32];
    key[31] = seed;
    let eoa = address_from_secret(&key).unwrap();
    let issued: Result<ChallengeResponse, ErrorCode> = update(
        pic,
        vault,
        caller,
        "issue_challenge",
        ChallengeRequest {
            eoa_address: eoa.to_vec().into(),
            principal: caller,
            purpose: ChallengePurpose::Login,
            network: Network::Local,
            origin: "https://load.example.test".into(),
        },
    )
    .unwrap();
    let issued = issued.unwrap();
    let challenge = private_perp::Challenge {
        purpose: "login".into(),
        eoa,
        principal: caller.as_slice().to_vec(),
        canister: vault.as_slice().to_vec(),
        network: "local".into(),
        origin: "https://load.example.test".into(),
        nonce: issued.nonce.as_ref().try_into().unwrap(),
        expires_at: issued.expires_at,
    };
    let opened: Result<api_types::auth::SessionHandle, ErrorCode> = update(
        pic,
        vault,
        caller,
        "open_session",
        OpenSessionRequest {
            challenge_id: issued.challenge_id,
            eoa_signature: challenge
                .sign_for_tests(&key)
                .unwrap()
                .to_bytes65()
                .to_vec()
                .into(),
        },
    )
    .unwrap();
    let opened = opened.unwrap();
    pocket_ic_tests::activate_local_user(pic, vault, caller, &opened);
    opened
}

fn route(
    call: &pocket_ic_tests::CapturedHttpCall,
    oid: u64,
) -> Result<(u16, Vec<u8>), (u64, String)> {
    let body: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
    if call.url.ends_with("/exchange") {
        return match body["action"]["type"].as_str() {
            Some("updateLeverage") | Some("approveAgent") | Some("usdSend") => Ok((200, ACCEPTED.to_vec())),
            Some("order") if body["action"].get("builder").is_none() => Ok((200, format!(r#"{{"status":"ok","response":{{"type":"order","data":{{"statuses":[{{"resting":{{"oid":{oid}}}}}]}}}}}}"#).into_bytes())),
            Some("order") => Err((1, "nonzero or unexpected builder fee".into())),
            Some("cancel") | Some("cancelByCloid") => Ok((200, br#"{"status":"ok","response":{"type":"cancel","data":{"statuses":["success"]}}}"#.to_vec())),
            other => Err((1, format!("unexpected exchange action {other:?}"))),
        };
    }
    match body["type"].as_str() {
        Some("metaAndAssetCtxs") => Ok((200, br#"[{"universe":[{"name":"SOL"},{"name":"ETH"},{"name":"BTC"}]},[{"dayNtlVlm":"10000000"},{"dayNtlVlm":"100000000"},{"dayNtlVlm":"500000000"}]]"#.to_vec())),
        Some("l2Book") => {
            let mid = if body["coin"] == "BTC" { 60000 } else { 3000 };
            Ok((200, format!(r#"{{"levels":[[{{"px":"{}","sz":"1.25"}}],[{{"px":"{}","sz":"1.10"}}]]}}"#, mid - 1, mid + 1).into_bytes()))
        },
        Some("clearinghouseState") => Ok((200, EMPTY_STATE.to_vec())),
        Some("openOrders") | Some("userFillsByTime") => Ok((200, b"[]".to_vec())),
        Some("orderStatus") => Ok((200, format!(r#"{{"status":"cancelled","order":{{"oid":{oid}}}}}"#).into_bytes())),
        other => Err((1, format!("unexpected info query {other:?}"))),
    }
}

fn p95(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[(values.len() * 95).div_ceil(100).saturating_sub(1)]
}

// Inclusive canister balance deltas for each workflow segment. These include
// concurrent timer work and must not be presented as isolated signing,
// outcall, or storage costs.
fn add_phase_cycles<const N: usize>(
    pic: &pocket_ic::PocketIc,
    watched: &[candid::Principal; N],
    before: &[u128; N],
    total: &mut [u128; N],
) -> [u128; N] {
    let mut after = [0u128; N];
    for (index, canister) in watched.iter().enumerate() {
        after[index] = pic.cycle_balance(*canister);
        total[index] = total[index].saturating_add(before[index].saturating_sub(after[index]));
    }
    after
}

fn run(users: usize) {
    let pic = pic();
    let controller = principal(200);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).unwrap();
    set.unwrap();
    let policy = configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    let set: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_meta_cache",
        (
            "local".to_string(),
            "hyperliquid".to_string(),
            UNIVERSE.to_string(),
        ),
    )
    .unwrap();
    set.unwrap();
    let set: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .unwrap();
    set.unwrap();
    rotate_hpke_key(&pic, core, controller);
    let journal_vault: Result<Option<candid::Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).unwrap();
    let journal_core: Result<Option<candid::Principal>, ErrorCode> =
        query(&pic, core, controller, "get_send_journal", ()).unwrap();
    let watched = [
        vault,
        core,
        policy,
        journal_vault.unwrap().unwrap(),
        journal_core.unwrap().unwrap(),
    ];
    let before: Vec<u128> = watched.iter().map(|id| pic.cycle_balance(*id)).collect();
    let mut accept_ms = Vec::with_capacity(users);
    let mut complete_ms = Vec::with_capacity(users);
    let mut posts = [0usize; 4]; // allocation, order, cancel, recovery
    let mut failure_count = 0usize;
    let mut budget_wait_ms = 0u64;
    let mut peak_rest_weight = 0u32;
    let mut total_rest_weight = 0u64;
    // login/funding, allocation, agent/observation, order, cancel, recovery
    let mut phase_cycles = [[0u128; 5]; 6];
    for index in 0..users {
        let budget: Result<RestBudgetStatus, ErrorCode> =
            query(&pic, policy, controller, "get_rest_budget_status", ()).unwrap();
        let budget = budget.unwrap();
        peak_rest_weight = peak_rest_weight.max(budget.used);
        if budget.used > 900 {
            total_rest_weight += u64::from(budget.used);
            pic.advance_time(std::time::Duration::from_secs(61));
            pic.tick();
            budget_wait_ms += 61_000;
        }
        let before_user: Result<RestBudgetStatus, ErrorCode> =
            query(&pic, policy, controller, "get_rest_budget_status", ()).unwrap();
        let before_user = before_user.unwrap().used;
        let start = Instant::now();
        let mut phase_before = watched.map(|id| pic.cycle_balance(id));
        let caller = principal(20 + index as u8);
        let session = session(&pic, vault, caller, 21 + index as u8);
        let request_id = format!("mixed-fixed-20260924-{index:03}");
        let trading_address = fund_trading_account(
            &pic,
            vault,
            controller,
            caller,
            &session,
            request_id.as_bytes(),
            5_000_000_000,
            index as u8 + 1,
        );
        phase_before = add_phase_cycles(&pic, &watched, &phase_before, &mut phase_cycles[0]);
        let (allocation, calls): (Result<u32, ErrorCode>, _) =
            call_with_routed_outcalls(&pic, vault, controller, "test_sweep_now", (), |call| {
                route(call, index as u64 + 10_000)
            })
            .unwrap();
        allocation.unwrap_or_else(|error| panic!("allocation user {index}: {error:?}"));
        posts[0] += calls
            .iter()
            .filter(|call| call.url.ends_with("/exchange"))
            .count();
        phase_before = add_phase_cycles(&pic, &watched, &phase_before, &mut phase_cycles[1]);

        let agent: Result<AgentGeneration, ErrorCode> = update(
            &pic,
            core,
            caller,
            "request_agent_generation",
            session.clone(),
        )
        .unwrap();
        let agent = agent.unwrap();
        approve_agent_at_vault(
            &pic,
            vault,
            caller,
            &session,
            1,
            agent.agent_address.as_ref(),
        )
        .unwrap();
        observe_empty_account(&pic, core, caller, &session);
        let account_id = trading_account_id(&pic, vault, caller, &session);
        phase_before = add_phase_cycles(&pic, &watched, &phase_before, &mut phase_cycles[2]);
        let accepted_at = Instant::now();
        let order: Result<SubmitOrderResult, ErrorCode> = update_args(
            &pic,
            core,
            caller,
            "submit_order",
            (
                session.clone(),
                SubmitOrderArgs {
                    session: session.clone(),
                    client_request_id: format!("mixed-order-{index:03}").into_bytes().into(),
                    account_id,
                    market: "ETH".into(),
                    side: Side::Buy,
                    kind: OrderKind::LimitGtc,
                    quantity: "0.01".into(),
                    limit_price: Some("2500".into()),
                    slippage_tolerance_bps: None,
                    reduce_only: false,
                    leverage: Some(3),
                    trigger: None,
                    expires_after: None,
                },
            ),
        )
        .unwrap();
        let order = order.unwrap();
        accept_ms.push(accepted_at.elapsed().as_millis());
        let oid = index as u64 + 10_000;
        let (swept, calls): (Result<SweepOutcome, ErrorCode>, _) =
            call_with_routed_outcalls(&pic, core, controller, "test_sweep_now", (), |call| {
                route(call, oid)
            })
            .unwrap();
        if swept.unwrap().dispatched != 1 {
            failure_count += 1;
        }
        posts[1] += calls
            .iter()
            .filter(|call| {
                call.url.ends_with("/exchange")
                    && serde_json::from_slice::<serde_json::Value>(&call.body)
                        .is_ok_and(|body| body["action"]["type"] == "order")
            })
            .count();
        phase_before = add_phase_cycles(&pic, &watched, &phase_before, &mut phase_cycles[3]);
        let canceled: Result<(), ErrorCode> =
            envelope::cancel_order(&pic, core, caller, &session, order.order_id).unwrap();
        canceled.unwrap();
        let (swept, calls): (Result<SweepOutcome, ErrorCode>, _) =
            call_with_routed_outcalls(&pic, core, controller, "test_sweep_now", (), |call| {
                route(call, oid)
            })
            .unwrap();
        if swept.unwrap().cancels != 1 {
            failure_count += 1;
        }
        posts[2] += calls
            .iter()
            .filter(|call| {
                call.url.ends_with("/exchange")
                    && serde_json::from_slice::<serde_json::Value>(&call.body).is_ok_and(|body| {
                        body["action"]["type"] == "cancel"
                            || body["action"]["type"] == "cancelByCloid"
                    })
            })
            .count();
        phase_before = add_phase_cycles(&pic, &watched, &phase_before, &mut phase_cycles[4]);
        let recovery: Result<FundRequestAccepted, ErrorCode> = update_args(
            &pic,
            vault,
            caller,
            "request_recovery",
            (
                session.clone(),
                Blob::from(format!("mixed-recovery-{index:03}").into_bytes()),
                1_000_000u64,
            ),
        )
        .unwrap();
        recovery.unwrap();
        let (swept, calls): (Result<u32, ErrorCode>, _) =
            call_with_routed_outcalls(&pic, vault, controller, "test_sweep_now", (), |call| {
                route(call, oid)
            })
            .unwrap();
        if swept.unwrap() != 1 {
            failure_count += 1;
        }
        posts[3] += calls
            .iter()
            .filter(|call| {
                call.url.ends_with("/exchange")
                    && serde_json::from_slice::<serde_json::Value>(&call.body)
                        .is_ok_and(|body| body["action"]["type"] == "usdSend")
            })
            .count();
        let status: Result<FundStatus, ErrorCode> =
            update(&pic, vault, caller, "get_fund_status", session).unwrap();
        let status = status.unwrap();
        if status.recovery_fence.is_some() || status.trading_equity != 4_999_000_000 {
            failure_count += 1;
        }
        let expected = format!("0x{}", hex::encode(trading_address));
        if !calls.iter().any(|call| {
            call.url.ends_with("/info") && String::from_utf8_lossy(&call.body).contains(&expected)
        }) {
            failure_count += 1;
        }
        add_phase_cycles(&pic, &watched, &phase_before, &mut phase_cycles[5]);
        complete_ms.push(start.elapsed().as_millis());
        let after_user: Result<RestBudgetStatus, ErrorCode> =
            query(&pic, policy, controller, "get_rest_budget_status", ()).unwrap();
        assert!(
            after_user.as_ref().unwrap().used >= before_user,
            "window reset inside one workflow"
        );
    }
    let budget: Result<RestBudgetStatus, ErrorCode> =
        query(&pic, policy, controller, "get_rest_budget_status", ()).unwrap();
    let budget = budget.unwrap();
    total_rest_weight += u64::from(budget.used);
    let eth: Result<MarketStatus, ErrorCode> = query(
        &pic,
        core,
        controller,
        "get_market_status",
        "ETH".to_string(),
    )
    .unwrap();
    let eth = eth.unwrap();
    let now_ms = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000;
    let market_age_ms = eth.observed_at.map(|at| now_ms.saturating_sub(at));
    let after: Vec<u128> = watched.iter().map(|id| pic.cycle_balance(*id)).collect();
    let consumed: Vec<u128> = before
        .iter()
        .zip(after)
        .map(|(before, after)| before.saturating_sub(after))
        .collect();
    eprintln!(
        "MIXED_LOAD {{\"seed\":20260924,\"users\":{users},\"allocation_posts\":{},\"order_posts\":{},\"cancel_posts\":{},\"recovery_posts\":{},\"rest_weight_total\":{total_rest_weight},\"rest_weight_current_window\":{},\"rest_weight_peak_sample\":{peak_rest_weight},\"budget_wait_ms\":{budget_wait_ms},\"order_accept_p95_host_ms\":{},\"workflow_p95_host_ms\":{},\"market_age_ms\":{},\"cycles_vault\":{},\"cycles_core\":{},\"cycles_policy\":{},\"cycles_vault_journal\":{},\"cycles_core_journal\":{},\"phase_cycles\":{},\"failures\":{failure_count}}}",
        posts[0],
        posts[1],
        posts[2],
        posts[3],
        budget.used,
        p95(accept_ms),
        p95(complete_ms),
        market_age_ms.unwrap_or(u64::MAX),
        consumed[0],
        consumed[1],
        consumed[2],
        consumed[3],
        consumed[4],
        serde_json::to_string(&phase_cycles).unwrap()
    );
    assert_eq!(posts, [users; 4]);
    assert_eq!(failure_count, 0);
}

#[test]
fn twenty_users() {
    run(20);
}

#[test]
fn hundred_users() {
    run(100);
}
