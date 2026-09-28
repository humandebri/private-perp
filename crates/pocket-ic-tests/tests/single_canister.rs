use api_types::error::ErrorCode;
use candid::{Principal, decode_one, encode_args};
use pocket_ic::PocketIc;
use pocket_ic_tests::{principal, query, update, update_args};

fn private<R: candid::CandidType + serde::de::DeserializeOwned>(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    role: &str,
    method: &str,
    payload: Vec<u8>,
) -> Result<R, ErrorCode> {
    let client = pocket_ic_tests::envelope::client(91);
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
        std::fs::read(&path).expect("read Wasm"),
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
    }
    // Install-code debit is replenished per execution round, not by jumping time.
    for _ in 0..200 {
        pic.tick();
    }
    pic.upgrade_canister(
        canister,
        std::fs::read(&path).unwrap(),
        encode_args(()).unwrap(),
        None,
    )
    .expect("upgrade");
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
        (2u64, vec!["BTC".to_string()]),
    )
    .unwrap();
    configured.unwrap();
}
