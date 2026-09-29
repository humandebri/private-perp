use api_types::error::ErrorCode;
use candid::{Principal, decode_one, encode_args};
use pocket_ic::PocketIc;
use pocket_ic_tests::{call_with_routed_outcalls, principal, query, update, update_args};

#[path = "support/predeploy.rs"]
mod predeploy;

fn private<R: candid::CandidType + serde::de::DeserializeOwned>(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    role: &str,
    method: &str,
    payload: Vec<u8>,
) -> Result<R, ErrorCode> {
    let client = pocket_ic_tests::envelope::client(91);
    let payload = if role == "core" {
        candid::encode_one(api_types::Blob::from(payload)).unwrap()
    } else {
        payload
    };
    let (request, aad) = client
        .prepare_encoded_for_role(pic, canister, caller, method, &payload, Some(role))
        .unwrap();
    let response: Result<api_types::envelope::HpkeResponse, ErrorCode> = update(
        pic,
        canister,
        caller,
        &format!("{role}_private_call"),
        request.clone(),
    )
    .unwrap();
    client.decode_encoded(response, &request, &aad).unwrap()
}

fn private_with_outcalls<R: candid::CandidType + serde::de::DeserializeOwned>(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    role: &str,
    method: &str,
    payload: Vec<u8>,
    route: impl Fn(&pocket_ic_tests::CapturedHttpCall) -> Result<(u16, Vec<u8>), (u64, String)>,
) -> (Result<R, ErrorCode>, Vec<pocket_ic_tests::CapturedHttpCall>) {
    let client = pocket_ic_tests::envelope::client(91);
    let payload = if role == "core" {
        candid::encode_one(api_types::Blob::from(payload)).unwrap()
    } else {
        payload
    };
    let (request, aad) = client
        .prepare_encoded_for_role(pic, canister, caller, method, &payload, Some(role))
        .unwrap();
    let (response, calls): (Result<api_types::envelope::HpkeResponse, ErrorCode>, _) =
        call_with_routed_outcalls(
            pic,
            canister,
            caller,
            &format!("{role}_private_call"),
            (request.clone(),),
            route,
        )
        .unwrap();
    (
        client.decode_encoded(response, &request, &aad).unwrap(),
        calls,
    )
}

#[test]
fn one_canister_preserves_authentication_and_journal_recovery() {
    let pic = pocket_ic_tests::pic();
    let admin = principal(51);
    let canister = pic.create_canister();
    pic.add_cycles(canister, 10_000_000_000_000_000);
    let path = std::env::var("PRIVATE_PERP_UNIFIED_WASM")
        .expect("set PRIVATE_PERP_UNIFIED_WASM to the built unified Wasm");
    pic.install_canister(
        canister,
        std::fs::read(std::env::var("PRIVATE_PERP_BASELINE_WASM").unwrap_or_else(|_| path.clone()))
            .expect("read initial Wasm"),
        encode_args((admin,)).unwrap(),
        None,
    );

    for method in [
        "vault_version",
        "core_version",
        "policy_version",
        "journal_version",
    ] {
        let bytes = pic
            .query_call(
                canister,
                Principal::anonymous(),
                method,
                encode_args(()).unwrap(),
            )
            .unwrap_or_else(|error| panic!("{method}: {error:?}"));
        assert_eq!(decode_one::<String>(&bytes).expect("version"), "0.1.0");
    }

    let response = pic
        .update_call(
            canister,
            Principal::anonymous(),
            "role_head",
            encode_args(("vault",)).unwrap(),
        )
        .expect("journal role request must return a typed denial");
    let result: Result<api_types::journal::JournalHead, ErrorCode> =
        decode_one(&response).expect("journal role response");
    assert!(matches!(result, Err(ErrorCode::Unauthenticated { .. })));

    for method in [
        "vault_get_policy_principal",
        "core_get_policy_principal",
        "get_core_principal",
        "get_vault_principal",
    ] {
        let linked: Option<Principal> = query(&pic, canister, admin, method, ()).unwrap();
        assert_eq!(linked, Some(canister), "{method}");
    }
    let budget = api_types::operations::RestBudgetConfig {
        capacity: 1000,
        exit_reserve: 100,
    };
    let denied: Result<(), ErrorCode> = update(
        &pic,
        canister,
        principal(52),
        "policy_configure_rest_budget",
        budget.clone(),
    )
    .unwrap();
    assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
    let configured: Result<(), ErrorCode> = update(
        &pic,
        canister,
        admin,
        "policy_configure_rest_budget",
        budget,
    )
    .unwrap();
    configured.unwrap();

    let saved_admin: Principal =
        query(&pic, canister, admin, "application_administrator", ()).unwrap();
    assert_eq!(saved_admin, admin);
    for role in ["vault", "core"] {
        let method = format!("{role}_configure_cycles");
        let denied: Result<(), ErrorCode> = update_args(
            &pic,
            canister,
            principal(52),
            &method,
            (100_000_000_000u128, 1_000_000_000_000u128),
        )
        .unwrap();
        assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
        let configured: Result<(), ErrorCode> = update_args(
            &pic,
            canister,
            admin,
            &method,
            (100_000_000_000u128, 1_000_000_000_000u128),
        )
        .unwrap();
        configured.unwrap();
    }
    let issuer_key = [9u8; 32];
    let issuer = hl_sign::address_from_secret(&issuer_key).unwrap();
    for (caller, allowed) in [(principal(52), false), (admin, true)] {
        let result: Result<(), ErrorCode> = update_args(
            &pic,
            canister,
            caller,
            "vault_configure_eligibility",
            (1u64, issuer.to_vec(), true),
        )
        .unwrap();
        assert_eq!(result.is_ok(), allowed);
        let result: Result<(), ErrorCode> = update(
            &pic,
            canister,
            caller,
            "core_configure_market_threshold",
            api_types::operations_status::MarketThreshold {
                market: "BTC".into(),
                expected_index: 0,
                min_day_notional_usdc: 1,
                max_spread_bps: 100,
                min_each_side_depth_usdc: 1,
            },
        )
        .unwrap();
        assert_eq!(result.is_ok(), allowed);
    }
    let configured: Result<(), ErrorCode> = update_args(
        &pic,
        canister,
        admin,
        "set_policy_version",
        (1u64, vec!["BTC".to_string()]),
    )
    .unwrap();
    configured.unwrap();

    if std::env::var("PRIVATE_PERP_PROFILE").as_deref() == Ok("empty") {
        predeploy::profile(&pic, canister, admin);
        return;
    }
    // A real signed login writes the vault's recovery journal through self-calls.
    let caller = principal(53);
    let secret = [7u8; 32];
    let eoa = hl_sign::address_from_secret(&secret).unwrap();
    let challenge: Result<api_types::auth::ChallengeResponse, ErrorCode> = update(
        &pic,
        canister,
        caller,
        "issue_challenge",
        api_types::auth::ChallengeRequest {
            eoa_address: eoa.to_vec().into(),
            principal: caller,
            purpose: api_types::auth::ChallengePurpose::Login,
            network: api_types::Network::Local,
            origin: "https://test.example".into(),
        },
    )
    .unwrap();
    let challenge = challenge.unwrap();
    let signature = hl_sign::private_perp::Challenge {
        purpose: "login".into(),
        eoa,
        principal: caller.as_slice().to_vec(),
        canister: canister.as_slice().to_vec(),
        network: "local".into(),
        origin: "https://test.example".into(),
        nonce: challenge.nonce.as_ref().try_into().unwrap(),
        expires_at: challenge.expires_at,
    }
    .sign_for_tests(&secret)
    .unwrap();
    let session: Result<api_types::auth::SessionHandle, ErrorCode> = update(
        &pic,
        canister,
        caller,
        "open_session",
        api_types::auth::OpenSessionRequest {
            challenge_id: challenge.challenge_id,
            eoa_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .unwrap();
    let session = session.unwrap();
    let owner: Result<api_types::Blob, ErrorCode> =
        update(&pic, canister, caller, "whoami", session.clone()).unwrap();
    assert!(!owner.unwrap().is_empty());
    let denied: Result<api_types::Blob, ErrorCode> =
        update(&pic, canister, principal(54), "whoami", session.clone()).unwrap();
    assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));

    let account: api_types::Blob = private(
        &pic,
        canister,
        caller,
        "vault",
        "prepare_trading_account",
        candid::encode_one(session.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(account.len(), 32);
    let expiry = pocket_ic_tests::envelope::now_ms(&pic) + 3_600_000;
    let claims: api_types::eligibility::EligibilityClaims = private(
        &pic,
        canister,
        caller,
        "vault",
        "eligibility_signing_claims",
        encode_args((session.clone(), expiry)).unwrap(),
    )
    .unwrap();
    let encoded = candid::encode_one(&claims).unwrap();
    let digest = hl_sign::keccak256_concat(&[b"private-perp/eligibility/v1", &encoded]);
    let signature = hl_sign::sign_digest_for_tests(&digest, &issuer_key).unwrap();
    let token = api_types::eligibility::EligibilityToken {
        claims,
        signature: signature.to_bytes65().to_vec().into(),
    };
    let status: api_types::eligibility::EligibilityStatus = private(
        &pic,
        canister,
        caller,
        "vault",
        "register_eligibility",
        encode_args((session.clone(), token)).unwrap(),
    )
    .unwrap();
    assert!(status.eligible);
    let allocation = api_types::fund::AllocationRequest {
        session: session.clone(),
        client_request_id: vec![31; 32].into(),
        amount: 400_000,
        target: api_types::AccountKind::Trading,
        intent_signature: None,
    };
    let no_funds: Result<api_types::fund::FundRequestAccepted, ErrorCode> = private(
        &pic,
        canister,
        caller,
        "vault",
        "request_allocation",
        candid::encode_one(&allocation).unwrap(),
    );
    assert!(
        matches!(no_funds, Err(ErrorCode::InsufficientFunds { .. })),
        "{no_funds:?}"
    );
    if std::env::var_os("PRIVATE_PERP_PROFILE").is_some() {
        let _: api_types::Blob = private(
            &pic,
            canister,
            caller,
            "vault",
            "provision_reserve_account",
            candid::encode_one(session.clone()).unwrap(),
        )
        .unwrap();
        predeploy::profile(&pic, canister, admin);
        return;
    }
    if std::env::var_os("PRIVATE_PERP_BASELINE_WASM").is_some() {
        let reserve: api_types::Blob = private(
            &pic,
            canister,
            caller,
            "vault",
            "provision_reserve_account",
            candid::encode_one(session.clone()).unwrap(),
        )
        .unwrap();
        let now = pocket_ic_tests::envelope::now_ms(&pic);
        let (credited, _): (Result<u32, ErrorCode>, _) =
            call_with_routed_outcalls(
                &pic,
                canister,
                Principal::anonymous(),
                "reconcile_deposits",
                (reserve.clone(),),
                |_| {
                    Ok((200, serde_json::json!([{
                "hash":format!("0x{}", "ab".repeat(32)), "time":now,
                "delta":{"type":"internalTransfer","user":format!("0x{}",hex::encode(eoa)),
                    "destination":format!("0x{}",hex::encode(reserve.as_ref())), "usdc":"1"}
            }]).to_string().into_bytes()))
                },
            )
            .unwrap();
        assert_eq!(credited.unwrap(), 1);
        let pending: api_types::fund::FundRequestAccepted = private(
            &pic,
            canister,
            caller,
            "vault",
            "request_allocation",
            candid::encode_one(&allocation).unwrap(),
        )
        .unwrap();
        assert_eq!(pending.state, api_types::fund::FundRequestState::Reserved);
    }
    if std::env::var_os("PRIVATE_PERP_UNIFIED_MOCK").is_none()
        && std::env::var_os("PRIVATE_PERP_BASELINE_WASM").is_none()
    {
        let reserve: api_types::Blob = private(
            &pic,
            canister,
            caller,
            "vault",
            "provision_reserve_account",
            candid::encode_one(session.clone()).unwrap(),
        )
        .unwrap();
        let denied: Result<(), ErrorCode> = private(
            &pic,
            canister,
            principal(54),
            "vault",
            "confirm_deposit",
            candid::encode_one(session.clone()).unwrap(),
        );
        assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
        let denied: Result<(), ErrorCode> = update(
            &pic,
            canister,
            caller,
            "refresh_trading_balance",
            session.clone(),
        )
        .unwrap();
        assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
        let stamp = pocket_ic_tests::envelope::now_ms(&pic);
        for _ in 0..2 {
            let (confirmed, calls): (Result<(), ErrorCode>, _) = private_with_outcalls(
                &pic,
                canister,
                caller,
                "vault",
                "confirm_deposit",
                candid::encode_one(session.clone()).unwrap(),
                |call| {
                    assert_eq!(
                        call.replication,
                        pocket_ic::common::rest::CanisterHttpReplication::NonReplicated
                    );
                    let request: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
                    assert_eq!(request["type"], "userNonFundingLedgerUpdates");
                    assert_eq!(
                        request["user"],
                        format!("0x{}", hex::encode(reserve.as_ref()))
                    );
                    Ok((
                        200,
                        serde_json::json!([{"hash":format!("0x{}", "aa".repeat(32)), "time":stamp,
                        "delta":{"type":"internalTransfer", "user":format!("0x{}",hex::encode(eoa)),
                        "destination":format!("0x{}",hex::encode(reserve.as_ref())), "usdc":"1"}}])
                        .to_string()
                        .into_bytes(),
                    ))
                },
            );
            confirmed.unwrap();
            assert_eq!(calls.len(), 1);
        }
        let funds: Result<api_types::fund::FundStatus, ErrorCode> =
            query(&pic, canister, caller, "get_fund_status", session.clone()).unwrap();
        assert_eq!(
            funds.unwrap().reserve_unallocated,
            1_000_000,
            "repeated confirmation must not credit twice"
        );
        // Let both workers quiesce, then prove a stored reserve creates no HTTP work.
        for _ in 0..3 {
            pic.advance_time(std::time::Duration::from_secs(5));
            for _ in 0..30 {
                pic.tick();
            }
        }
        let idle_balance = pic.cycle_balance(canister);
        pic.advance_time(std::time::Duration::from_secs(600));
        for _ in 0..50 {
            pic.tick();
        }
        assert!(
            pic.get_canister_http().is_empty(),
            "holding reserve must not poll HL"
        );
        assert!(
            idle_balance - pic.cycle_balance(canister) < 20_000_000,
            "no repeating idle timer"
        );
        // UI cycle status reads write samples but must not restart idle timers.
        for method in ["vault_get_cycles_status", "core_get_cycles_status"] {
            let status: Result<api_types::operations_status::CyclesStatus, ErrorCode> =
                update(&pic, canister, caller, method, ()).unwrap();
            status.unwrap();
        }
        for _ in 0..30 {
            pic.tick();
        }
        let status_idle_balance = pic.cycle_balance(canister);
        pic.advance_time(std::time::Duration::from_secs(5));
        for _ in 0..50 {
            pic.tick();
        }
        assert!(pic.get_canister_http().is_empty());
        assert!(
            status_idle_balance - pic.cycle_balance(canister) < 20_000_000,
            "status sampling must not restart idle workers"
        );

        let cleared: Result<(), ErrorCode> =
            update(&pic, canister, admin, "clear_emergency_stop", ()).unwrap();
        cleared.unwrap();
        let requested: api_types::fund::FundRequestAccepted = private(
            &pic,
            canister,
            caller,
            "vault",
            "request_allocation",
            candid::encode_one(allocation.clone()).unwrap(),
        )
        .unwrap();
        assert_eq!(requested.state, api_types::fund::FundRequestState::Reserved);
        pic.advance_time(std::time::Duration::from_secs(5));
        let mut sends = 0;
        for _ in 0..100 {
            pic.tick();
            for request in pic.get_canister_http() {
                assert!(request.url.ends_with("/exchange"));
                sends += 1;
                pic.mock_canister_http_response(
                    pocket_ic::common::rest::MockCanisterHttpResponse {
                        subnet_id: request.subnet_id,
                        request_id: request.request_id,
                        response: pocket_ic::common::rest::CanisterHttpResponse::CanisterHttpReply(
                            pocket_ic::common::rest::CanisterHttpReply {
                                status: 200,
                                headers: vec![],
                                body: br#"{"status":"ok","response":{"type":"default"}}"#.to_vec(),
                            },
                        ),
                        additional_responses: vec![],
                    },
                );
            }
        }
        assert_eq!(
            sends, 1,
            "a committed allocation must wake a stopped worker"
        );
        let funds: Result<api_types::fund::FundStatus, ErrorCode> =
            query(&pic, canister, caller, "get_fund_status", session.clone()).unwrap();
        assert_eq!(funds.unwrap().reserve_unallocated, 600_000);
        pic.advance_time(std::time::Duration::from_secs(5));
        let mut arrival_checks = 0;
        for _ in 0..100 {
            pic.tick();
            for request in pic.get_canister_http() {
                assert_eq!(
                    request.replication,
                    pocket_ic::common::rest::CanisterHttpReplication::NonReplicated
                );
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                assert_eq!(body["type"], "userNonFundingLedgerUpdates");
                arrival_checks += 1;
                pic.mock_canister_http_response(pocket_ic::common::rest::MockCanisterHttpResponse {
                    subnet_id:request.subnet_id,request_id:request.request_id,
                    response:pocket_ic::common::rest::CanisterHttpResponse::CanisterHttpReply(
                        pocket_ic::common::rest::CanisterHttpReply {status:200,headers:vec![],body:serde_json::json!([
                            {"hash":format!("0x{}","bb".repeat(32)),"time":pocket_ic_tests::envelope::now_ms(&pic),
                            "delta":{"type":"internalTransfer","user":format!("0x{}",hex::encode(reserve.as_ref())),
                                "destination":body["user"],"usdc":"0.4"}}]).to_string().into_bytes()}),additional_responses:vec![],
                });
            }
        }
        assert_eq!(
            arrival_checks, 1,
            "only the pending allocation destination is observed"
        );

        pic.advance_time(std::time::Duration::from_secs(60));
        for _ in 0..50 {
            pic.tick();
        }
        assert!(
            pic.get_canister_http().is_empty(),
            "settled allocation must stop the worker"
        );
        let configured: Result<(), ErrorCode> = update(
            &pic,
            canister,
            admin,
            "core_configure_market_threshold",
            api_types::operations_status::MarketThreshold {
                market: "ETH".into(),
                expected_index: 1,
                min_day_notional_usdc: 1,
                max_spread_bps: 100,
                min_each_side_depth_usdc: 1,
            },
        )
        .unwrap();
        configured.unwrap();
        let rotated: Result<Vec<u8>, ErrorCode> = update(
            &pic,
            canister,
            Principal::anonymous(),
            "core_rotate_hpke_key",
            (),
        )
        .unwrap();
        rotated.unwrap();
        let (refreshed, calls): (Result<(), ErrorCode>, _) = private_with_outcalls(
            &pic,
            canister,
            caller,
            "core",
            "refresh_trading",
            candid::encode_one(session.clone()).unwrap(),
            |call| {
                assert_eq!(
                    call.replication,
                    pocket_ic::common::rest::CanisterHttpReplication::NonReplicated
                );
                let request: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
                let body = match request["type"].as_str().unwrap() {
                    "metaAndAssetCtxs" => {
                        serde_json::json!([{"universe":[{"name":"BTC"},{"name":"ETH"}]},[{"dayNtlVlm":"100"},{"dayNtlVlm":"100"}]])
                    }
                    "l2Book" => {
                        serde_json::json!({"coin":request["coin"], "time":pocket_ic_tests::envelope::now_ms(&pic),
                        "levels":[[{"px":"99.9","sz":"1"}],[{"px":"100.1","sz":"1"}]]})
                    }
                    "clearinghouseState" => {
                        serde_json::json!({"marginSummary":{"accountValue":"0.4","totalMarginUsed":"0.1"},
                        "assetPositions":[{"position":{"coin":"ETH","szi":"0.001","entryPx":"100","liquidationPx":"50",
                            "unrealizedPnl":"0","leverage":{"value":3},"marginMode":"cross"}}], "withdrawable":"0.3"})
                    }
                    "userFills" => serde_json::json!([]),
                    other => panic!("unexpected explicit refresh {other}"),
                };
                Ok((200, body.to_string().into_bytes()))
            },
        );
        refreshed.unwrap();
        assert!(
            calls.len() >= 6,
            "explicit refresh must fetch market and account state"
        );
        pic.advance_time(std::time::Duration::from_secs(5));
        let mut observed = 0;
        for _ in 0..100 {
            pic.tick();
            for request in pic.get_canister_http() {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                assert_eq!(body["type"], "clearinghouseState");
                observed += 1;
                pic.mock_canister_http_response(pocket_ic::common::rest::MockCanisterHttpResponse {
                    subnet_id:request.subnet_id, request_id:request.request_id,
                    response:pocket_ic::common::rest::CanisterHttpResponse::CanisterHttpReply(
                        pocket_ic::common::rest::CanisterHttpReply {status:200,headers:vec![],
                            body:br#"{"assetPositions":[],"marginSummary":{"accountValue":"0.4","totalMarginUsed":"0"},"withdrawable":"0.4"}"#.to_vec()}),
                    additional_responses:vec![],
                });
            }
        }
        assert_eq!(observed, 1, "an open position must keep monitoring alive");
        pic.advance_time(std::time::Duration::from_secs(10));
        for _ in 0..30 {
            pic.tick();
        }
        let quiet = pic.cycle_balance(canister);
        pic.advance_time(std::time::Duration::from_secs(600));
        for _ in 0..50 {
            pic.tick();
        }
        assert!(
            pic.get_canister_http().is_empty(),
            "a closed position must stop monitoring"
        );
        assert!(quiet - pic.cycle_balance(canister) < 20_000_000);
        let mut unknown = allocation.clone();
        unknown.client_request_id = vec![92; 32].into();
        unknown.amount = 100_000;
        let _: api_types::fund::FundRequestAccepted = private(
            &pic,
            canister,
            caller,
            "vault",
            "request_allocation",
            candid::encode_one(unknown).unwrap(),
        )
        .unwrap();
        let mut posts = 0;
        let mut checks = 0;
        for _ in 0..24 {
            pic.advance_time(std::time::Duration::from_secs(5));
            for _ in 0..50 {
                pic.tick();
                for request in pic.get_canister_http() {
                    assert_eq!(
                        request.replication,
                        pocket_ic::common::rest::CanisterHttpReplication::NonReplicated
                    );
                    if request.url.ends_with("/exchange") {
                        posts += 1;
                    } else {
                        checks += 1;
                    }
                    pic.mock_canister_http_response(
                        pocket_ic::common::rest::MockCanisterHttpResponse {
                            subnet_id: request.subnet_id,
                            request_id: request.request_id,
                            response:
                                pocket_ic::common::rest::CanisterHttpResponse::CanisterHttpReply(
                                    pocket_ic::common::rest::CanisterHttpReply {
                                        status: 503,
                                        headers: vec![],
                                        body: b"unavailable".to_vec(),
                                    },
                                ),
                            additional_responses: vec![],
                        },
                    );
                }
            }
        }
        assert_eq!(posts, 1, "unknown POST must never be retried");
        assert_eq!(checks, 0, "unknown sends require manual result checks");
        let work: Vec<(String, Vec<u8>, u64)> = private(
            &pic,
            canister,
            caller,
            "vault",
            "get_manual_work",
            candid::encode_one(session.clone()).unwrap(),
        )
        .unwrap();
        let item = work
            .iter()
            .find(|(kind, _, _)| kind == "fund")
            .expect("stopped fund task")
            .clone();
        let payload = candid::encode_args((
            session.clone(),
            item.0.clone(),
            api_types::Blob::from(item.1.clone()),
            item.2,
        ))
        .unwrap();
        let denied: Result<(), ErrorCode> = private(
            &pic,
            canister,
            principal(199),
            "vault",
            "resume_manual_work",
            payload.clone(),
        );
        assert!(denied.is_err(), "another principal cannot resume the task");
        let resumed: Result<(), ErrorCode> = private(
            &pic,
            canister,
            caller,
            "vault",
            "resume_manual_work",
            payload.clone(),
        );
        resumed.unwrap();
        let duplicate: Result<(), ErrorCode> = private(
            &pic,
            canister,
            caller,
            "vault",
            "resume_manual_work",
            payload,
        );
        assert!(duplicate.is_err(), "one permission cannot be granted twice");
        let mut manual_checks = 0;
        for _ in 0..24 {
            pic.advance_time(std::time::Duration::from_secs(5));
            for _ in 0..50 {
                pic.tick();
                for request in pic.get_canister_http() {
                    assert!(
                        !request.url.ends_with("/exchange"),
                        "manual result check must not resend"
                    );
                    manual_checks += 1;
                    let running: Result<(), ErrorCode> = private(
                        &pic,
                        canister,
                        caller,
                        "vault",
                        "resume_manual_work",
                        candid::encode_args((
                            session.clone(),
                            item.0.clone(),
                            api_types::Blob::from(item.1.clone()),
                            item.2 + 1,
                        ))
                        .unwrap(),
                    );
                    assert!(running.is_err(), "in-flight attempts cannot be resumed");
                    pic.mock_canister_http_response(
                        pocket_ic::common::rest::MockCanisterHttpResponse {
                            subnet_id: request.subnet_id,
                            request_id: request.request_id,
                            response:
                                pocket_ic::common::rest::CanisterHttpResponse::CanisterHttpReply(
                                    pocket_ic::common::rest::CanisterHttpReply {
                                        status: 503,
                                        headers: vec![],
                                        body: b"unavailable".to_vec(),
                                    },
                                ),
                            additional_responses: vec![],
                        },
                    );
                }
            }
        }
        assert_eq!(
            manual_checks, 1,
            "manual permission allows exactly one failed observation"
        );
        let stale: Result<(), ErrorCode> = private(
            &pic,
            canister,
            caller,
            "vault",
            "resume_manual_work",
            candid::encode_args((
                session.clone(),
                item.0.clone(),
                api_types::Blob::from(item.1.clone()),
                item.2,
            ))
            .unwrap(),
        );
        assert!(
            stale.is_err(),
            "an old generation cannot restart a newer failed attempt"
        );
        let quiet = pic.cycle_balance(canister);
        pic.advance_time(std::time::Duration::from_secs(60));
        for _ in 0..50 {
            pic.tick();
        }
        assert!(pic.get_canister_http().is_empty());
        assert!(
            quiet - pic.cycle_balance(canister) < 20_000_000,
            "unresolved task must not keep a timer alive"
        );
    }
    if std::env::var_os("PRIVATE_PERP_UNIFIED_MOCK").is_some() {
        let credit: Result<(), ErrorCode> = update_args(
            &pic,
            canister,
            caller,
            "test_credit_deposit",
            (session.clone(), 1_000_000u64, vec![32u8; 32]),
        )
        .unwrap();
        credit.unwrap();
        for expected in [
            api_types::fund::FundRequestState::Reserved,
            api_types::fund::FundRequestState::Accepted,
        ] {
            let accepted: api_types::fund::FundRequestAccepted = private(
                &pic,
                canister,
                caller,
                "vault",
                "request_allocation",
                candid::encode_one(&allocation).unwrap(),
            )
            .unwrap();
            assert_eq!(accepted.state, expected);
            assert_eq!(accepted.request_id, allocation.client_request_id);
        }
        let funds: Result<api_types::fund::FundStatus, ErrorCode> =
            query(&pic, canister, caller, "get_fund_status", session.clone()).unwrap();
        assert_eq!(funds.unwrap().reserve_unallocated, 1_000_000);
        let mut next = allocation.clone();
        next.client_request_id = vec![33; 32].into();
        next.amount = 600_001;
        let over: Result<api_types::fund::FundRequestAccepted, ErrorCode> = private(
            &pic,
            canister,
            caller,
            "vault",
            "request_allocation",
            candid::encode_one(&next).unwrap(),
        );
        assert!(
            matches!(over, Err(ErrorCode::InsufficientFunds { .. })),
            "{over:?}"
        );
        next.amount = 600_000;
        let remaining: api_types::fund::FundRequestAccepted = private(
            &pic,
            canister,
            caller,
            "vault",
            "request_allocation",
            candid::encode_one(&next).unwrap(),
        )
        .unwrap();
        assert_eq!(remaining.state, api_types::fund::FundRequestState::Reserved);

        // Drive the integrated custody and trading send paths. These HTTP
        // replies are mock HL responses; no real testnet POST is implied.
        let credit: Result<(), ErrorCode> = update_args(
            &pic,
            canister,
            caller,
            "test_credit_deposit",
            (session.clone(), 10_000_000u64, vec![34u8; 32]),
        )
        .unwrap();
        credit.unwrap();
        let trading_allocation = api_types::fund::AllocationRequest {
            session: session.clone(),
            client_request_id: vec![35; 32].into(),
            amount: 5_000_000,
            target: api_types::AccountKind::Trading,
            intent_signature: None,
        };
        let allocated: api_types::fund::FundRequestAccepted = private(
            &pic,
            canister,
            caller,
            "vault",
            "request_allocation",
            candid::encode_one(trading_allocation).unwrap(),
        )
        .unwrap();
        assert_eq!(allocated.state, api_types::fund::FundRequestState::Reserved);
        let configured: Result<(), ErrorCode> =
            update(&pic, canister, admin, "clear_emergency_stop", ()).unwrap();
        configured.unwrap();
        let (sent, transfer_calls): (Result<u32, ErrorCode>, _) =
            call_with_routed_outcalls(&pic, canister, admin, "vault_test_sweep_now", (), |call| {
                assert!(call.url.ends_with("/exchange"));
                let body: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
                assert_eq!(body["action"]["type"], "usdSend");
                Ok((
                    200,
                    br#"{"status":"ok","response":{"type":"default"}}"#.to_vec(),
                ))
            })
            .unwrap();
        assert_eq!(sent.unwrap(), 3);
        assert_eq!(transfer_calls.len(), 3);
        let trading_address: Result<api_types::Blob, ErrorCode> = update(
            &pic,
            canister,
            caller,
            "get_trading_address",
            session.clone(),
        )
        .unwrap();
        let credited: Result<bool, ErrorCode> = update_args(
            &pic,
            canister,
            Principal::anonymous(),
            "credit_venue_deposit",
            (
                api_types::Blob::from(vec![36u8; 32]),
                5_000_000u64,
                trading_address.unwrap(),
                "usdc".to_string(),
            ),
        )
        .unwrap();
        assert!(credited.unwrap());
        let funds: Result<api_types::fund::FundStatus, ErrorCode> =
            query(&pic, canister, caller, "get_fund_status", session.clone()).unwrap();
        assert_eq!(funds.unwrap().trading_equity, 5_000_000);

        let configured: Result<(), ErrorCode> = update_args(
            &pic,
            canister,
            admin,
            "core_configure_market_threshold",
            (api_types::operations_status::MarketThreshold {
                market: "ETH".into(),
                expected_index: 1,
                min_day_notional_usdc: 1_000_000,
                max_spread_bps: 20,
                min_each_side_depth_usdc: 1_000,
            },),
        )
        .unwrap();
        configured.unwrap();
        let configured: Result<(), ErrorCode> = update_args(
            &pic,
            canister,
            Principal::anonymous(),
            "set_market_context",
            ("local".to_string(), "hyperliquid".to_string()),
        )
        .unwrap();
        configured.unwrap();
        let configured: Result<(), ErrorCode> = update_args(
            &pic,
            canister,
            Principal::anonymous(),
            "set_meta_cache",
            (
                "local".to_string(),
                "hyperliquid".to_string(),
                r#"[{"name":"SOL","szDecimals":0},{"name":"ETH","szDecimals":5},{"name":"BTC","szDecimals":5}]"#.to_string(),
            ),
        )
        .unwrap();
        configured.unwrap();
        let configured: Result<(), ErrorCode> = update_args(
            &pic,
            canister,
            admin,
            "set_policy_version",
            (2u64, vec!["BTC".to_string(), "ETH".to_string()]),
        )
        .unwrap();
        configured.unwrap();
        let configured: Result<(), ErrorCode> =
            update(&pic, canister, admin, "clear_emergency_stop", ()).unwrap();
        configured.unwrap();
        let (refreshed, market_calls): (Result<(), ErrorCode>, _) = call_with_routed_outcalls(
            &pic,
            canister,
            Principal::anonymous(),
            "refresh_market",
            (),
            |call| {
                let query: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
                match query["type"].as_str() {
                    Some("metaAndAssetCtxs") => Ok((200, br#"[{"universe":[{"name":"SOL"},{"name":"ETH"},{"name":"BTC"}]},[{"dayNtlVlm":"10000000"},{"dayNtlVlm":"100000000"},{"dayNtlVlm":"500000000"}]]"#.to_vec())),
                    Some("l2Book") => Ok((200, serde_json::json!({
                        "coin": query["coin"],
                        "time": pic.get_time().as_nanos_since_unix_epoch() / 1_000_000,
                        "levels": [[{"px":"2499","sz":"1.25"}],[{"px":"2501","sz":"1.10"}]]
                    }).to_string().into_bytes())),
                    other => Err((1, format!("unexpected market query: {other:?}"))),
                }
            },
        )
        .unwrap();
        refreshed.unwrap();
        assert!(!market_calls.is_empty());
        let rotated: Result<Vec<u8>, ErrorCode> = update(
            &pic,
            canister,
            Principal::anonymous(),
            "core_rotate_hpke_key",
            (),
        )
        .unwrap();
        rotated.unwrap();
        let agent: api_types::fund::AgentGeneration = private(
            &pic,
            canister,
            caller,
            "core",
            "request_agent_generation",
            candid::encode_one(session.clone()).unwrap(),
        )
        .unwrap();
        let (approved, approval_calls): (Result<api_types::fund::AgentGeneration, ErrorCode>, _) =
            private_with_outcalls(
                &pic,
                canister,
                caller,
                "vault",
                "approve_agent_generation",
                encode_args((session.clone(), 1u64, agent.agent_address)).unwrap(),
                |call| {
                    assert!(call.url.ends_with("/exchange"));
                    let body: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
                    assert_eq!(body["action"]["type"], "approveAgent");
                    Ok((
                        200,
                        br#"{"status":"ok","response":{"type":"default"}}"#.to_vec(),
                    ))
                },
            );
        assert_eq!(approved.unwrap().state, api_types::fund::AgentState::Active);
        assert_eq!(approval_calls.len(), 1);
        let observed: Result<u32, ErrorCode> = update_args(
            &pic,
            canister,
            caller,
            "test_ingest_positions",
            (
                session.clone(),
                r#"{"assetPositions":[],"marginSummary":{"totalMarginUsed":"0"}}"#.to_string(),
            ),
        )
        .unwrap();
        assert_eq!(observed.unwrap(), 0);
        let order = api_types::order::SubmitOrderArgs {
            session: session.clone(),
            client_request_id: vec![37; 32].into(),
            account_id: account.clone(),
            market: "ETH".into(),
            side: api_types::order::Side::Buy,
            kind: api_types::order::OrderKind::LimitGtc,
            quantity: "0.001".into(),
            limit_price: Some("2500".into()),
            slippage_tolerance_bps: None,
            reduce_only: false,
            leverage: Some(3),
            trigger: None,
            expires_after: None,
        };
        let accepted: api_types::order::SubmitOrderResult = private(
            &pic,
            canister,
            caller,
            "core",
            "submit_order",
            encode_args((session.clone(), order)).unwrap(),
        )
        .unwrap();
        assert_eq!(accepted.request_id.as_ref(), &[37; 32]);
        let (swept, order_calls): (Result<api_types::order::SweepOutcome, ErrorCode>, _) =
            call_with_routed_outcalls(
                &pic,
                canister,
                admin,
                "core_test_sweep_now",
                (),
                |call| {
                    if call.url.ends_with("/exchange") {
                        let body: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
                        return match body["action"]["type"].as_str() {
                            Some("updateLeverage") => Ok((200, br#"{"status":"ok","response":{"type":"default"}}"#.to_vec())),
                            Some("order") => Ok((200, br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":4242}}]}}}"#.to_vec())),
                            other => Err((1, format!("unexpected exchange action: {other:?}"))),
                        };
                    }
                    let body: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
                    match body["type"].as_str() {
                        Some("clearinghouseState") => Ok((200, br#"{"marginSummary":{"accountValue":"100","totalMarginUsed":"0"},"assetPositions":[],"withdrawable":"50"}"#.to_vec())),
                        Some("userFills") => Ok((200, b"[]".to_vec())),
                        Some("orderStatus") => Ok((200, br#"{"status":"open","order":{"oid":4242,"coin":"ETH","sz":"0.001"}}"#.to_vec())),
                        other => Err((1, format!("unexpected info query: {other:?}"))),
                    }
                },
            )
            .unwrap();
        assert_eq!(swept.unwrap().dispatched, 1);
        assert!(order_calls.iter().any(|call| {
            serde_json::from_slice::<serde_json::Value>(&call.body)
                .ok()
                .is_some_and(|body| body["action"]["type"] == "order")
        }));
    }
    // Install-code debit is replenished per execution round, not by jumping time.
    for _ in 0..200 {
        pic.tick();
    }
    let before: Result<api_types::fund::FundStatus, ErrorCode> =
        query(&pic, canister, caller, "get_fund_status", session.clone()).unwrap();
    let before = before.unwrap();
    let recovery_snapshot = std::env::var_os("PRIVATE_PERP_BASELINE_WASM").map(|_| {
        pic.take_canister_snapshot(canister, None, None)
            .expect("pre-upgrade snapshot")
    });
    let stopped_before: Option<Vec<(String, Vec<u8>, u64)>> =
        if std::env::var_os("PRIVATE_PERP_BASELINE_WASM").is_none() {
            Some(
                private(
                    &pic,
                    canister,
                    caller,
                    "vault",
                    "get_manual_work",
                    candid::encode_one(session.clone()).unwrap(),
                )
                .unwrap(),
            )
        } else {
            None
        };
    pic.upgrade_canister(
        canister,
        std::fs::read(&path).unwrap(),
        encode_args(()).unwrap(),
        None,
    )
    .expect("upgrade");
    let stopped_after: Vec<(String, Vec<u8>, u64)> = private(
        &pic,
        canister,
        caller,
        "vault",
        "get_manual_work",
        candid::encode_one(session.clone()).unwrap(),
    )
    .unwrap();
    if let Some(stopped_before) = stopped_before {
        assert_eq!(
            stopped_before, stopped_after,
            "upgrade must preserve stopped permissions"
        );
    }
    let saved_admin: Principal =
        query(&pic, canister, admin, "application_administrator", ()).unwrap();
    assert_eq!(saved_admin, admin);
    let after: Result<api_types::fund::FundStatus, ErrorCode> =
        query(&pic, canister, caller, "get_fund_status", session.clone()).unwrap();
    assert_eq!(
        before,
        after.unwrap(),
        "balances and reservations must survive the upgrade"
    );
    let owner: Result<api_types::Blob, ErrorCode> =
        update(&pic, canister, caller, "whoami", session.clone()).unwrap();
    assert!(!owner.unwrap().is_empty(), "session must survive");
    let denied: Result<api_types::Blob, ErrorCode> =
        update(&pic, canister, principal(54), "whoami", session.clone()).unwrap();
    assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
    for role in ["vault", "core"] {
        let status: Result<(bool, bool), ErrorCode> = query(
            &pic,
            canister,
            admin,
            &format!("{role}_get_journal_send_status"),
            (),
        )
        .unwrap();
        assert!(status.unwrap().0, "upgrade must stop sends for {role}");
        let denied: Result<(), ErrorCode> = update(
            &pic,
            canister,
            Principal::anonymous(),
            &format!("{role}_resume_journal"),
            (),
        )
        .unwrap();
        assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
        let resumed: Result<(), ErrorCode> =
            update(&pic, canister, admin, &format!("{role}_resume_journal"), ()).unwrap();
        resumed.unwrap();
    }
    let configured: Result<(), ErrorCode> = update_args(
        &pic,
        canister,
        admin,
        "set_policy_version",
        (
            if std::env::var_os("PRIVATE_PERP_UNIFIED_MOCK").is_some() {
                3u64
            } else {
                2u64
            },
            vec!["BTC".to_string()],
        ),
    )
    .unwrap();
    configured.unwrap();
    if let Some(snapshot) = recovery_snapshot {
        if std::env::var_os("PRIVATE_PERP_ROLLBACK").is_some() {
            // No exchange side effect has occurred in this isolated fixture.
            pic.load_canister_snapshot(canister, None, snapshot.id)
                .expect("restore old snapshot");
            let restored: Result<api_types::fund::FundStatus, ErrorCode> =
                query(&pic, canister, caller, "get_fund_status", session.clone()).unwrap();
            assert_eq!(before, restored.unwrap());
            let restored_admin: Principal =
                query(&pic, canister, admin, "application_administrator", ()).unwrap();
            assert_eq!(restored_admin, admin);
            return;
        }
        let cleared: Result<(), ErrorCode> =
            update(&pic, canister, admin, "clear_emergency_stop", ()).unwrap();
        cleared.unwrap();
        // Idle throttling must not delay a reserved allocation by one minute.
        pic.advance_time(std::time::Duration::from_secs(5));
        let mut sends = 0;
        for _ in 0..100 {
            pic.tick();
            for request in pic.get_canister_http() {
                let body = if request.url.ends_with("/exchange") {
                    sends += 1;
                    br#"{"status":"ok","response":{"type":"default"}}"#.to_vec()
                } else {
                    b"[]".to_vec()
                };
                pic.mock_canister_http_response(
                    pocket_ic::common::rest::MockCanisterHttpResponse {
                        subnet_id: request.subnet_id,
                        request_id: request.request_id,
                        response: pocket_ic::common::rest::CanisterHttpResponse::CanisterHttpReply(
                            pocket_ic::common::rest::CanisterHttpReply {
                                status: 200,
                                headers: vec![],
                                body,
                            },
                        ),
                        additional_responses: vec![],
                    },
                );
            }
        }
        assert_eq!(
            sends, 1,
            "pending allocation must dispatch on the first 5-second timer"
        );
    }
}
