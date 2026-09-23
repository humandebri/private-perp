//! Shared REST budget authority, rolling limits, exit reserve and persistence.
use api_types::error::ErrorCode;
use api_types::operations::{BudgetClass, RestBudgetConfig, RestBudgetRequest, RestBudgetStatus};
use candid::Principal;
use pocket_ic_tests::{POLICY_WASM, deploy, pic, principal, query, update, update_args, wasm};

fn setup(pic: &pocket_ic::PocketIc) -> Principal {
    let policy = deploy(
        pic,
        POLICY_WASM,
        Some(vec![principal(60)]),
        candid::encode_one(()).unwrap(),
    );
    for (method, who) in [
        ("set_operator", 61),
        ("set_sns_principal", 62),
        ("set_guard_principal", 63),
    ] {
        let result: Result<(), ErrorCode> =
            update(pic, policy, principal(60), method, principal(who)).unwrap();
        result.unwrap();
    }
    for (role, who) in [("vault", 64), ("core", 65)] {
        let result: Result<(), ErrorCode> = update_args(
            pic,
            policy,
            principal(60),
            "register_budget_worker",
            (role.to_string(), principal(who)),
        )
        .unwrap();
        result.unwrap();
    }
    let result: Result<(), ErrorCode> = update(
        pic,
        policy,
        principal(63),
        "configure_rest_budget",
        RestBudgetConfig {
            capacity: 1200,
            exit_reserve: 300,
        },
    )
    .unwrap();
    result.unwrap();
    let result: Result<(), ErrorCode> =
        update(pic, policy, principal(62), "clear_emergency_stop", ()).unwrap();
    result.unwrap();
    policy
}

fn request(
    pic: &pocket_ic::PocketIc,
    id: u8,
    class: BudgetClass,
    weight: u32,
) -> RestBudgetRequest {
    let expires_at = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000 + 50_000;
    let mut request_id = vec![id; 32];
    request_id[..8].copy_from_slice(&expires_at.to_be_bytes());
    RestBudgetRequest {
        request_id: request_id.into(),
        class,
        weight,
        expires_at,
    }
}

fn consume(
    pic: &pocket_ic::PocketIc,
    policy: Principal,
    who: u8,
    req: RestBudgetRequest,
) -> Result<(), ErrorCode> {
    update(pic, policy, principal(who), "consume_rest_budget", req).unwrap()
}

fn status(pic: &pocket_ic::PocketIc, policy: Principal) -> RestBudgetStatus {
    let result: Result<RestBudgetStatus, ErrorCode> =
        query(pic, policy, principal(99), "get_rest_budget_status", ()).unwrap();
    result.unwrap()
}

#[test]
fn shared_limit_reserves_exits_and_never_refunds_grants() {
    let pic = pic();
    let p = setup(&pic);
    let req = request(&pic, 1, BudgetClass::NewRisk, 600);
    consume(&pic, p, 65, req.clone()).unwrap();
    assert_eq!(
        consume(&pic, p, 65, req),
        Err(ErrorCode::ReservationConflict)
    );
    consume(&pic, p, 64, request(&pic, 2, BudgetClass::NewRisk, 300)).unwrap();
    assert!(matches!(
        consume(&pic, p, 65, request(&pic, 3, BudgetClass::NewRisk, 1)),
        Err(ErrorCode::VenueRateLimited { .. })
    ));
    consume(&pic, p, 64, request(&pic, 4, BudgetClass::Exit, 200)).unwrap();
    consume(&pic, p, 65, request(&pic, 5, BudgetClass::Reconcile, 100)).unwrap();
    assert!(matches!(
        consume(&pic, p, 64, request(&pic, 6, BudgetClass::Exit, 1)),
        Err(ErrorCode::VenueRateLimited { .. })
    ));
    assert_eq!(status(&pic, p).used, 1200);
    pic.advance_time(std::time::Duration::from_secs(61));
    pic.tick();
    assert_eq!(status(&pic, p).used, 1200);
    pic.advance_time(std::time::Duration::from_secs(50));
    pic.tick();
    assert_eq!(status(&pic, p).used, 0);
    consume(&pic, p, 65, request(&pic, 7, BudgetClass::NewRisk, 900)).unwrap();
}

#[test]
fn only_registered_workers_consume_and_roles_cannot_bypass_stops() {
    let pic = pic();
    let p = setup(&pic);
    for who in [60, 61, 62, 63, 99] {
        assert!(matches!(
            consume(&pic, p, who, request(&pic, who, BudgetClass::Exit, 1)),
            Err(ErrorCode::Unauthenticated { .. })
        ));
    }
    let denied: Result<(), ErrorCode> = update_args(
        &pic,
        p,
        principal(99),
        "register_budget_worker",
        ("core".to_string(), principal(99)),
    )
    .unwrap();
    assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
    let replace: Result<(), ErrorCode> = update_args(
        &pic,
        p,
        principal(60),
        "register_budget_worker",
        ("core".to_string(), principal(99)),
    )
    .unwrap();
    assert_eq!(replace, Err(ErrorCode::ReservationConflict));
    let denied: Result<(), ErrorCode> = update(
        &pic,
        p,
        principal(61),
        "configure_rest_budget",
        RestBudgetConfig {
            capacity: 2000,
            exit_reserve: 500,
        },
    )
    .unwrap();
    assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
    let stop: Result<(), ErrorCode> =
        update(&pic, p, principal(61), "set_emergency_stop", ()).unwrap();
    stop.unwrap();
    assert_eq!(
        consume(&pic, p, 65, request(&pic, 1, BudgetClass::NewRisk, 1)),
        Err(ErrorCode::PolicyUnavailable)
    );
    consume(&pic, p, 64, request(&pic, 2, BudgetClass::Exit, 1)).unwrap();
    let pause: Result<(), ErrorCode> =
        update(&pic, p, principal(61), "pause_for_recovery", ()).unwrap();
    pause.unwrap();
    assert_eq!(
        consume(&pic, p, 64, request(&pic, 3, BudgetClass::Exit, 1)),
        Err(ErrorCode::ReservationConflict)
    );
    consume(&pic, p, 65, request(&pic, 4, BudgetClass::Reconcile, 1)).unwrap();
    let denied: Result<(), ErrorCode> =
        update(&pic, p, principal(61), "clear_recovery_pause", ()).unwrap();
    assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
}

#[test]
fn upgrade_and_reconfiguration_do_not_reset_consumption_or_pause() {
    let pic = pic();
    let p = setup(&pic);
    consume(&pic, p, 65, request(&pic, 1, BudgetClass::NewRisk, 800)).unwrap();
    let pause: Result<(), ErrorCode> =
        update(&pic, p, principal(61), "pause_for_recovery", ()).unwrap();
    pause.unwrap();
    pic.upgrade_canister(
        p,
        wasm(POLICY_WASM),
        candid::encode_one(()).unwrap(),
        Some(principal(60)),
    )
    .unwrap();
    let config: Result<(), ErrorCode> = update(
        &pic,
        p,
        principal(63),
        "configure_rest_budget",
        RestBudgetConfig {
            capacity: 1000,
            exit_reserve: 250,
        },
    )
    .unwrap();
    config.unwrap();
    assert_eq!(status(&pic, p).used, 800);
    assert!(status(&pic, p).recovery_paused);
    let resume: Result<(), ErrorCode> =
        update(&pic, p, principal(62), "clear_recovery_pause", ()).unwrap();
    resume.unwrap();
    assert!(matches!(
        consume(&pic, p, 65, request(&pic, 2, BudgetClass::NewRisk, 1)),
        Err(ErrorCode::VenueRateLimited { .. })
    ));
    consume(&pic, p, 64, request(&pic, 3, BudgetClass::Exit, 200)).unwrap();
}

#[test]
fn malformed_expired_and_unconfigured_requests_fail_closed() {
    let pic = pic();
    let p = setup(&pic);
    for config in [
        RestBudgetConfig {
            capacity: 3,
            exit_reserve: 1,
        },
        RestBudgetConfig {
            capacity: 1200,
            exit_reserve: 0,
        },
        RestBudgetConfig {
            capacity: 1200,
            exit_reserve: 1200,
        },
        RestBudgetConfig {
            capacity: 10_001,
            exit_reserve: 1,
        },
    ] {
        let result: Result<(), ErrorCode> =
            update(&pic, p, principal(63), "configure_rest_budget", config).unwrap();
        assert!(matches!(result, Err(ErrorCode::BadRequest { .. })));
    }
    assert_eq!(status(&pic, p).config.unwrap().capacity, 1200);
    for (id, weight, expires_at) in [
        (vec![0; 31], 1, u64::MAX),
        (vec![1; 32], 0, u64::MAX),
        (vec![2; 32], 1, 0),
        (vec![3; 32], 1, u64::MAX),
    ] {
        assert!(matches!(
            consume(
                &pic,
                p,
                65,
                RestBudgetRequest {
                    request_id: id.into(),
                    class: BudgetClass::Reconcile,
                    weight,
                    expires_at
                }
            ),
            Err(ErrorCode::BadRequest { .. })
        ));
    }
    assert_eq!(status(&pic, p).used, 0);
    let empty = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![principal(60)]),
        candid::encode_one(()).unwrap(),
    );
    let registered: Result<(), ErrorCode> = update_args(
        &pic,
        empty,
        principal(60),
        "register_budget_worker",
        ("core".to_string(), principal(65)),
    )
    .unwrap();
    registered.unwrap();
    assert!(status(&pic, empty).config.is_none());
    assert!(status(&pic, empty).recovery_paused);
    let unconfigured_request = request(&pic, 8, BudgetClass::Exit, 1);
    assert_eq!(
        consume(&pic, empty, 65, unconfigured_request.clone()),
        Err(ErrorCode::PolicyUnavailable),
    );
    assert_eq!(status(&pic, empty).used, 0);
    let guard: Result<(), ErrorCode> = update(
        &pic,
        empty,
        principal(60),
        "set_guard_principal",
        principal(63),
    )
    .unwrap();
    guard.unwrap();
    let configured: Result<(), ErrorCode> = update(
        &pic,
        empty,
        principal(63),
        "configure_rest_budget",
        RestBudgetConfig {
            capacity: 1200,
            exit_reserve: 300,
        },
    )
    .unwrap();
    configured.unwrap();
    // The same registered caller and request succeeds once only configuration changes.
    consume(&pic, empty, 65, unconfigured_request).unwrap();
    assert_eq!(status(&pic, empty).used, 1);
}

#[test]
fn concurrent_workers_cannot_overspend_the_shared_window() {
    let pic = pic();
    let p = setup(&pic);
    let calls = [(64, 1), (65, 2)].map(|(who, id)| {
        pic.submit_call(
            p,
            principal(who),
            "consume_rest_budget",
            candid::encode_one(request(&pic, id, BudgetClass::NewRisk, 600)).unwrap(),
        )
        .unwrap()
    });
    let results = calls.map(|id| {
        candid::decode_one::<Result<(), ErrorCode>>(&pic.await_call(id).unwrap()).unwrap()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(ErrorCode::VenueRateLimited { .. })))
            .count(),
        1
    );
    assert_eq!(status(&pic, p).used, 600);
}

#[test]
fn grants_expire_individually_and_collected_ids_cannot_be_revived() {
    let pic = pic();
    let p = setup(&pic);
    let old = request(&pic, 1, BudgetClass::NewRisk, 500);
    consume(&pic, p, 65, old.clone()).unwrap();
    pic.advance_time(std::time::Duration::from_secs(30));
    pic.tick();
    consume(&pic, p, 64, request(&pic, 2, BudgetClass::NewRisk, 400)).unwrap();
    pic.advance_time(std::time::Duration::from_secs(81));
    pic.tick();
    assert_eq!(status(&pic, p).used, 400);
    // A successful grant collects the first, expired row.
    consume(&pic, p, 65, request(&pic, 3, BudgetClass::Exit, 1)).unwrap();
    assert!(matches!(
        consume(&pic, p, 65, old.clone()),
        Err(ErrorCode::BadRequest { .. })
    ));
    let mut forged = old;
    forged.expires_at = request(&pic, 4, BudgetClass::Exit, 1).expires_at;
    assert!(matches!(
        consume(&pic, p, 65, forged),
        Err(ErrorCode::BadRequest { .. })
    ));
    assert_eq!(status(&pic, p).used, 401);
}

#[test]
fn delayed_dispatch_stays_charged_for_a_full_window_after_its_deadline() {
    let pic = pic();
    let p = setup(&pic);
    let mut delayed = request(&pic, 1, BudgetClass::NewRisk, 900);
    delayed.expires_at = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000 + 60_000;
    let mut id = delayed.request_id.to_vec();
    id[..8].copy_from_slice(&delayed.expires_at.to_be_bytes());
    delayed.request_id = id.into();
    consume(&pic, p, 65, delayed.clone()).unwrap();

    // Model a worker checking a successful grant immediately before a late dispatch.
    pic.advance_time(std::time::Duration::from_secs(59));
    pic.tick();
    assert!(delayed.valid_at(pic.get_time().as_nanos_since_unix_epoch() / 1_000_000));
    pic.advance_time(std::time::Duration::from_secs(2));
    pic.tick();
    assert!(!delayed.valid_at(pic.get_time().as_nanos_since_unix_epoch() / 1_000_000));
    assert_eq!(status(&pic, p).new_risk_used, 900);
    // The t=59 dispatch must not be followed by a second full grant at t=61.
    assert!(matches!(
        consume(&pic, p, 64, request(&pic, 2, BudgetClass::NewRisk, 900)),
        Err(ErrorCode::VenueRateLimited { .. }),
    ));
    // A smaller exit grant runs GC: it must NOT remove the delayed grant's charge.
    consume(&pic, p, 64, request(&pic, 3, BudgetClass::Exit, 1)).unwrap();
    assert_eq!(status(&pic, p).used, 901);
    pic.advance_time(std::time::Duration::from_secs(58));
    pic.tick();
    assert_eq!(status(&pic, p).used, 901);
    // Once deadline+60 seconds has passed, only the newer exit grant remains.
    pic.advance_time(std::time::Duration::from_secs(2));
    pic.tick();
    assert_eq!(status(&pic, p).used, 1);
    consume(&pic, p, 65, request(&pic, 4, BudgetClass::NewRisk, 900)).unwrap();
}
