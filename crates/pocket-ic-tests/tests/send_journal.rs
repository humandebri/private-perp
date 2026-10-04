use api_types::Network;
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
    SessionStatus,
};
use api_types::error::ErrorCode;
use api_types::journal::{
    JournalHead, JournalRecord, RecoveryEvent, RecoveryPayload, RecoveryRecord, SendIntent,
};
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic_tests::{
    CONTROL_GUARD_WASM, FUNDS_VAULT_WASM, SEND_JOURNAL_WASM, TRADING_CORE_WASM, deploy, pic,
    principal, query, update, update_args,
};

fn intent(nonce: u64) -> SendIntent {
    SendIntent {
        kind: "order".into(),
        request_id: vec![1; 32].into(),
        account_id: vec![2; 32].into(),
        nonce,
        digest: vec![3; 32].into(),
    }
}

fn event(id: u8) -> RecoveryEvent {
    RecoveryEvent {
        version: 1,
        logical_id: vec![id; 32].into(),
        payload: RecoveryPayload::Reservation {
            request_id: vec![id; 32].into(),
            user_id: vec![4; 32].into(),
            account_id: vec![5; 32].into(),
            amount_micros: 123,
            state: "reserved".into(),
        },
    }
}

#[test]
fn guard_replays_vault_identity_but_keeps_sends_locked_without_baseline() {
    let pic = pic();
    let sns = principal(245);
    let owner = principal(246);
    let guard = deploy(
        &pic,
        CONTROL_GUARD_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let journal: Result<Option<candid::Principal>, ErrorCode> =
        query(&pic, vault, sns, "get_send_journal", ()).unwrap();
    let journal = journal.unwrap().unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, guard, sns, "set_sns_principal", sns).unwrap();
    configured.unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, vault, sns, "set_journal_guard", guard).unwrap();
    configured.unwrap();

    let mut secret = [0u8; 32];
    secret[31] = 71;
    let eoa = address_from_secret(&secret).unwrap();
    let user_id = [8u8; 32];
    let mut id_material = b"vault_identity".to_vec();
    id_material.extend_from_slice(b"local");
    id_material.extend_from_slice(&eoa);
    let identity = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&id_material).to_vec().into(),
        payload: RecoveryPayload::IdentityRegistration {
            user_id: user_id.to_vec().into(),
            owner,
            eoa_address: eoa.to_vec().into(),
            network: "local".into(),
        },
    };
    let appended: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append_recovery_event", identity).unwrap();
    assert_eq!(appended.unwrap().sequence, 1);
    let account_id = [7u8; 32];
    let address = [9u8; 20];
    let mut account_material = b"custody_account".to_vec();
    account_material.extend_from_slice(&user_id);
    account_material.extend_from_slice(b"trading");
    let account = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&account_material).to_vec().into(),
        payload: RecoveryPayload::CustodyAccount {
            user_id: Some(user_id.to_vec().into()),
            account_id: account_id.to_vec().into(),
            kind: "trading".into(),
            derivation_path: format!("private-perp/trading/{}", hex::encode(account_id)),
            address: address.to_vec().into(),
            network: "local".into(),
        },
    };
    let appended: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append_recovery_event", account).unwrap();
    assert_eq!(appended.unwrap().sequence, 2);
    let resumed: Result<(), ErrorCode> = update(&pic, guard, sns, "resume_journal", vault).unwrap();
    assert!(resumed.is_err(), "mapping replay is not full validation");
    let recovery: Result<(u64, bool), ErrorCode> =
        query(&pic, vault, sns, "recovery_stage_status", ()).unwrap();
    assert_eq!(recovery.unwrap(), (2, true));
    let pending: Result<bool, ErrorCode> =
        query(&pic, vault, sns, "recovery_replay_pending", ()).unwrap();
    assert!(pending.unwrap());

    let origin = "https://app.example.test";
    let issued: Result<ChallengeResponse, ErrorCode> = update(
        &pic,
        vault,
        owner,
        "issue_challenge",
        ChallengeRequest {
            eoa_address: eoa.to_vec().into(),
            principal: owner,
            purpose: ChallengePurpose::Login,
            network: Network::Local,
            origin: origin.into(),
        },
    )
    .unwrap();
    let issued = issued.unwrap();
    let challenge = private_perp::Challenge {
        purpose: "login".into(),
        eoa,
        principal: owner.as_slice().to_vec(),
        canister: vault.as_slice().to_vec(),
        network: "local".into(),
        origin: origin.into(),
        nonce: issued.nonce.as_ref().try_into().unwrap(),
        expires_at: issued.expires_at,
    };
    let signature = challenge.sign_for_tests(&secret).unwrap();
    let session: Result<SessionHandle, ErrorCode> = update(
        &pic,
        vault,
        owner,
        "open_session",
        OpenSessionRequest {
            challenge_id: issued.challenge_id,
            eoa_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .unwrap();
    let session = session.unwrap();
    let status: Result<SessionStatus, ErrorCode> =
        query(&pic, vault, owner, "session_status", session.clone()).unwrap();
    assert_eq!(status.unwrap().user_id.as_ref(), user_id);
    let restored_account: Result<Option<api_types::Blob>, ErrorCode> =
        query(&pic, vault, owner, "get_trading_account", session.clone()).unwrap();
    assert_eq!(restored_account.unwrap().unwrap().as_ref(), account_id);
    let restored_address: Result<api_types::Blob, ErrorCode> =
        query(&pic, vault, owner, "get_trading_address", session).unwrap();
    assert_eq!(restored_address.unwrap().as_ref(), address);
    let repeated: Result<(), ErrorCode> =
        update(&pic, guard, sns, "resume_journal", vault).unwrap();
    assert!(repeated.is_err(), "matching heads cannot clear replay hold");
}

#[test]
fn foreign_network_custody_replay_stays_staged_without_creating_an_account() {
    let pic = pic();
    let sns = principal(239);
    let owner = principal(240);
    let guard = deploy(
        &pic,
        CONTROL_GUARD_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let journal: Result<Option<candid::Principal>, ErrorCode> =
        query(&pic, vault, sns, "get_send_journal", ()).unwrap();
    let journal = journal.unwrap().unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, guard, sns, "set_sns_principal", sns).unwrap();
    configured.unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, vault, sns, "set_journal_guard", guard).unwrap();
    configured.unwrap();

    let mut secret = [0u8; 32];
    secret[31] = 72;
    let eoa = address_from_secret(&secret).unwrap();
    let user_id = [10u8; 32];
    let account_id = [11u8; 32];
    let mut id_material = b"vault_identity".to_vec();
    id_material.extend_from_slice(b"local");
    id_material.extend_from_slice(&eoa);
    let identity = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&id_material).to_vec().into(),
        payload: RecoveryPayload::IdentityRegistration {
            user_id: user_id.to_vec().into(),
            owner,
            eoa_address: eoa.to_vec().into(),
            network: "local".into(),
        },
    };
    let appended: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append_recovery_event", identity).unwrap();
    assert_eq!(appended.unwrap().sequence, 1);
    let mut account_material = b"custody_account".to_vec();
    account_material.extend_from_slice(&user_id);
    account_material.extend_from_slice(b"trading");
    let invalid_account = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&account_material).to_vec().into(),
        payload: RecoveryPayload::CustodyAccount {
            user_id: Some(user_id.to_vec().into()),
            account_id: account_id.to_vec().into(),
            kind: "trading".into(),
            derivation_path: format!("private-perp/trading/{}", hex::encode(account_id)),
            address: vec![12; 20].into(),
            network: "testnet".into(),
        },
    };
    let appended: Result<JournalHead, ErrorCode> = update(
        &pic,
        journal,
        vault,
        "append_recovery_event",
        invalid_account,
    )
    .unwrap();
    assert_eq!(appended.unwrap().sequence, 2);
    let resumed: Result<(), ErrorCode> = update(&pic, guard, sns, "resume_journal", vault).unwrap();
    assert!(resumed.is_err());
    let staged: Result<(u64, bool), ErrorCode> =
        query(&pic, vault, sns, "recovery_stage_status", ()).unwrap();
    assert_eq!(staged.unwrap(), (2, true));

    let origin = "https://app.example.test";
    let issued: Result<ChallengeResponse, ErrorCode> = update(
        &pic,
        vault,
        owner,
        "issue_challenge",
        ChallengeRequest {
            eoa_address: eoa.to_vec().into(),
            principal: owner,
            purpose: ChallengePurpose::Login,
            network: Network::Local,
            origin: origin.into(),
        },
    )
    .unwrap();
    let issued = issued.unwrap();
    let challenge = private_perp::Challenge {
        purpose: "login".into(),
        eoa,
        principal: owner.as_slice().to_vec(),
        canister: vault.as_slice().to_vec(),
        network: "local".into(),
        origin: origin.into(),
        nonce: issued.nonce.as_ref().try_into().unwrap(),
        expires_at: issued.expires_at,
    };
    let signature = challenge.sign_for_tests(&secret).unwrap();
    let session: Result<SessionHandle, ErrorCode> = update(
        &pic,
        vault,
        owner,
        "open_session",
        OpenSessionRequest {
            challenge_id: issued.challenge_id,
            eoa_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .unwrap();
    let account: Result<Option<api_types::Blob>, ErrorCode> =
        query(&pic, vault, owner, "get_trading_account", session.unwrap()).unwrap();
    assert!(account.unwrap().is_none());
    let repeated: Result<(), ErrorCode> =
        update(&pic, guard, sns, "resume_journal", vault).unwrap();
    assert!(repeated.is_err());
}

#[test]
fn unmatched_deposit_without_account_mapping_stays_staged() {
    let pic = pic();
    let sns = principal(246);
    let guard = deploy(
        &pic,
        CONTROL_GUARD_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let journal: Result<Option<candid::Principal>, ErrorCode> =
        query(&pic, vault, sns, "get_send_journal", ()).unwrap();
    let journal = journal.unwrap().unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, guard, sns, "set_sns_principal", sns).unwrap();
    configured.unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, vault, sns, "set_journal_guard", guard).unwrap();
    configured.unwrap();
    let tx_hash = [41u8; 32];
    let mut event_material = b"deposit".to_vec();
    event_material.extend_from_slice(&tx_hash);
    let event_id = hl_sign::keccak256(&event_material);
    let mut logical = b"deposit_credit".to_vec();
    logical.extend_from_slice(b"local");
    logical.extend_from_slice(&event_id);
    let event = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical).to_vec().into(),
        payload: RecoveryPayload::DepositCreditWithFee {
            sender: None,
            tx_hash: tx_hash.to_vec().into(),
            network: "local".into(),
            address: vec![42u8; 20].into(),
            amount_micros: 1_000_000,
            fee_micros: 0,
            observed_at_ms: 1,
        },
    };
    let appended: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append_recovery_event", event).unwrap();
    assert_eq!(appended.unwrap().sequence, 1);
    let replay: Result<(), ErrorCode> = update(&pic, guard, sns, "resume_journal", vault).unwrap();
    assert!(replay.is_err());
    let staged: Result<(u64, bool), ErrorCode> =
        query(&pic, vault, sns, "recovery_stage_status", ()).unwrap();
    assert_eq!(staged.unwrap(), (1, true));
    let pending: Result<bool, ErrorCode> =
        query(&pic, vault, sns, "recovery_replay_pending", ()).unwrap();
    assert!(!pending.unwrap(), "unmatched credit was not posted");
}

#[test]
fn guard_replays_core_account_mapping_but_keeps_sends_locked_without_baseline() {
    let pic = pic();
    let sns = principal(251);
    let owner = principal(252);
    let guard = deploy(
        &pic,
        CONTROL_GUARD_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let journal: Result<Option<candid::Principal>, ErrorCode> =
        query(&pic, core, sns, "get_send_journal", ()).unwrap();
    let journal = journal.unwrap().unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, guard, sns, "set_sns_principal", sns).unwrap();
    configured.unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, core, sns, "set_journal_guard", guard).unwrap();
    configured.unwrap();

    let account_id = [7u8; 32];
    let mut id_material = b"core_account_identity".to_vec();
    id_material.extend_from_slice(&account_id);
    let event = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&id_material).to_vec().into(),
        payload: RecoveryPayload::IdentityAccount {
            user_id: vec![8; 32].into(),
            owner,
            account_id: account_id.to_vec().into(),
            address: vec![9; 20].into(),
        },
    };
    let appended: Result<JournalHead, ErrorCode> =
        update(&pic, journal, core, "append_recovery_event", event).unwrap();
    assert_eq!(appended.unwrap().sequence, 1);
    let resumed: Result<(), ErrorCode> = update(&pic, guard, sns, "resume_journal", core).unwrap();
    assert!(
        resumed.is_err(),
        "account replay is not full backup validation"
    );
    let status: Result<(u64, u64, bool), ErrorCode> =
        query(&pic, core, sns, "journal_restore_status", ()).unwrap();
    assert_eq!(status.unwrap(), (0, 0, true));
    let recovery: Result<(u64, bool), ErrorCode> =
        query(&pic, core, sns, "recovery_stage_status", ()).unwrap();
    assert_eq!(recovery.unwrap(), (1, true));
    let pending: Result<bool, ErrorCode> =
        query(&pic, core, sns, "recovery_replay_pending", ()).unwrap();
    assert!(pending.unwrap());
    let repeated: Result<(), ErrorCode> = update(&pic, guard, sns, "resume_journal", core).unwrap();
    assert!(
        repeated.is_err(),
        "matching heads must not clear replay validation"
    );
    let foreign_user = [55u8; 32];
    let request_id = b"foreign-risk";
    let cloid = [10u8; 16];
    let mut order_material = cloid.to_vec();
    order_material.extend_from_slice(&foreign_user);
    let mut logical = b"order_accepted".to_vec();
    logical.extend_from_slice(&foreign_user);
    logical.extend_from_slice(request_id);
    let foreign_order = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical).to_vec().into(),
        payload: RecoveryPayload::OrderAccepted {
            order_id: hl_sign::keccak256(&order_material).to_vec().into(),
            request_id: request_id.to_vec().into(),
            user_id: foreign_user.to_vec().into(),
            account_id: account_id.to_vec().into(),
            cloid: cloid.to_vec().into(),
            body_hash: vec![11; 32].into(),
            risk_micros: 100_000,
            reduce_only: false,
            accepted_at_ms: 1,
        },
    };
    let appended: Result<JournalHead, ErrorCode> =
        update(&pic, journal, core, "append_recovery_event", foreign_order).unwrap();
    assert_eq!(appended.unwrap().sequence, 2);
    let rejected: Result<(), ErrorCode> = update(&pic, guard, sns, "resume_journal", core).unwrap();
    assert!(
        rejected.is_err(),
        "another user's account cannot gain a risk hold"
    );
    let recovery: Result<(u64, bool), ErrorCode> =
        query(&pic, core, sns, "recovery_stage_status", ()).unwrap();
    assert_eq!(recovery.unwrap(), (2, true), "foreign event stays staged");
}

#[test]
fn guard_keeps_core_locked_for_account_event_with_wrong_logical_id() {
    let pic = pic();
    let sns = principal(253);
    let guard = deploy(
        &pic,
        CONTROL_GUARD_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let journal: Result<Option<candid::Principal>, ErrorCode> =
        query(&pic, core, sns, "get_send_journal", ()).unwrap();
    let journal = journal.unwrap().unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, guard, sns, "set_sns_principal", sns).unwrap();
    configured.unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, core, sns, "set_journal_guard", guard).unwrap();
    configured.unwrap();
    let event = RecoveryEvent {
        version: 1,
        logical_id: vec![3; 32].into(),
        payload: RecoveryPayload::IdentityAccount {
            user_id: vec![4; 32].into(),
            owner: principal(254),
            account_id: vec![5; 32].into(),
            address: vec![6; 20].into(),
        },
    };
    let appended: Result<JournalHead, ErrorCode> =
        update(&pic, journal, core, "append_recovery_event", event).unwrap();
    appended.unwrap();
    let resumed: Result<(), ErrorCode> = update(&pic, guard, sns, "resume_journal", core).unwrap();
    assert!(resumed.is_err());
    let status: Result<(u64, u64, bool), ErrorCode> =
        query(&pic, core, sns, "journal_restore_status", ()).unwrap();
    assert_eq!(status.unwrap(), (0, 0, true));
    let recovery: Result<(u64, bool), ErrorCode> =
        query(&pic, core, sns, "recovery_stage_status", ()).unwrap();
    assert_eq!(recovery.unwrap(), (1, true));
    let pending: Result<bool, ErrorCode> =
        query(&pic, core, sns, "recovery_replay_pending", ()).unwrap();
    assert!(!pending.unwrap(), "the invalid event was never applied");
}

#[test]
fn private_recovery_stream_is_idempotent_and_isolated() {
    let pic = pic();
    let controller = principal(231);
    let vault = principal(232);
    let core = principal(233);
    let stranger = principal(234);
    let journal = deploy(
        &pic,
        SEND_JOURNAL_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    for (role, worker) in [("vault", vault), ("core", core)] {
        let result: Result<(), ErrorCode> = update_args(
            &pic,
            journal,
            controller,
            "register_worker",
            (role.to_string(), worker),
        )
        .unwrap();
        result.unwrap();
    }
    let denied: Result<JournalHead, ErrorCode> =
        update(&pic, journal, stranger, "append_recovery_event", event(1)).unwrap();
    assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
    let first: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append_recovery_event", event(1)).unwrap();
    let first = first.unwrap();
    assert_eq!(first.sequence, 1);
    let repeated: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append_recovery_event", event(1)).unwrap();
    assert_eq!(repeated.unwrap(), first);
    let mut changed = event(1);
    if let RecoveryPayload::Reservation { amount_micros, .. } = &mut changed.payload {
        *amount_micros = 124;
    }
    let conflict: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append_recovery_event", changed).unwrap();
    assert!(matches!(conflict, Err(ErrorCode::ReservationConflict)));
    for payload in [
        RecoveryPayload::IdentityRegistration {
            user_id: vec![7; 32].into(),
            owner: vault,
            eoa_address: vec![8; 20].into(),
            network: "local".into(),
        },
        RecoveryPayload::CustodyAccount {
            user_id: Some(vec![7; 32].into()),
            account_id: vec![8; 32].into(),
            kind: "trading".into(),
            derivation_path: format!("private-perp/trading/{}", hex::encode([8u8; 32])),
            address: vec![4; 20].into(),
            network: "local".into(),
        },
        RecoveryPayload::DepositCreditWithFee {
            sender: None,
            tx_hash: vec![3; 32].into(),
            network: "local".into(),
            address: vec![4; 20].into(),
            amount_micros: 1,
            fee_micros: 0,
            observed_at_ms: 1,
        },
        RecoveryPayload::DepositClaim {
            event_id: vec![5; 32].into(),
            user_id: vec![6; 32].into(),
            amount_micros: 1,
            claimed_at_ms: 1,
        },
        RecoveryPayload::FillObserved {
            tid: 1,
            hl_oid: 777,
            user_id: vec![6; 32].into(),
            order_id: vec![7; 32].into(),
            account_id: vec![8; 32].into(),
            market: "ETH".into(),
            quantity: "0.05".into(),
            price: "2500".into(),
            fee: 1,
            filled_at_ms: 1,
        },
        RecoveryPayload::OrderStatusObserved {
            order_id: vec![7; 32].into(),
            account_id: vec![8; 32].into(),
            hl_oid: 777,
            state: "cancelled".into(),
            evidence_digest: vec![6; 32].into(),
            observed_at_ms: 1,
        },
        RecoveryPayload::RecoverySettlement {
            action_id: vec![7; 32].into(),
            request_id: vec![8; 32].into(),
            user_id: vec![6; 32].into(),
            trading_account_id: vec![5; 32].into(),
            amount_micros: 1,
            nonce: 1,
            accepted: true,
            evidence_digest: vec![4; 32].into(),
            observed_at_ms: 1,
        },
        RecoveryPayload::RecoveryPostResult {
            action_id: vec![7; 32].into(),
            request_id: vec![8; 32].into(),
            user_id: vec![6; 32].into(),
            trading_account_id: vec![5; 32].into(),
            amount_micros: 1,
            nonce: 1,
            accepted: true,
            evidence_digest: vec![4; 32].into(),
            observed_at_ms: 1,
        },
        RecoveryPayload::FundTransferResult {
            action_id: vec![7; 32].into(),
            request_id: vec![8; 32].into(),
            user_id: vec![6; 32].into(),
            source_account_id: vec![5; 32].into(),
            destination: vec![4; 20].into(),
            kind: "withdrawal".into(),
            amount_micros: 1,
            nonce: 1,
            accepted: true,
            evidence_digest: vec![3; 32].into(),
            observed_at_ms: 1,
        },
        RecoveryPayload::OrderActionResult {
            order_id: vec![7; 32].into(),
            account_id: vec![8; 32].into(),
            client_request_id: vec![6; 32].into(),
            kind: "order".into(),
            accepted: true,
            hl_oid: Some(777),
            filled: false,
            observed_at_ms: 1,
        },
    ] {
        let invalid: Result<JournalHead, ErrorCode> = update(
            &pic,
            journal,
            vault,
            "append_recovery_event",
            RecoveryEvent {
                version: 1,
                logical_id: vec![9; 32].into(),
                payload,
            },
        )
        .unwrap();
        assert!(matches!(invalid, Err(ErrorCode::BadRequest { .. })));
    }
    let user_id = [6u8; 32];
    let request_id = b"wrong-order-identity";
    let mut logical = b"order_accepted".to_vec();
    logical.extend_from_slice(&user_id);
    logical.extend_from_slice(request_id);
    let invalid_order: Result<JournalHead, ErrorCode> = update(
        &pic,
        journal,
        core,
        "append_recovery_event",
        RecoveryEvent {
            version: 1,
            logical_id: hl_sign::keccak256(&logical).to_vec().into(),
            payload: RecoveryPayload::OrderAccepted {
                order_id: vec![7; 32].into(),
                request_id: request_id.to_vec().into(),
                user_id: user_id.to_vec().into(),
                account_id: vec![8; 32].into(),
                cloid: vec![9; 16].into(),
                body_hash: vec![10; 32].into(),
                risk_micros: 1,
                reduce_only: false,
                accepted_at_ms: 1,
            },
        },
    )
    .unwrap();
    assert!(matches!(invalid_order, Err(ErrorCode::BadRequest { .. })));
    let second: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append_recovery_event", event(2)).unwrap();
    let second = second.unwrap();
    let vault_records: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, vault, "recovery_events", (0u64, 10u32)).unwrap();
    let vault_records = vault_records.unwrap();
    assert_eq!(vault_records.len(), 2);
    assert_eq!(vault_records[1].previous_hash, first.hash);
    assert_eq!(vault_records[1].hash, second.hash);
    let core_records: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, core, "recovery_events", (0u64, 10u32)).unwrap();
    assert!(core_records.unwrap().is_empty());
    let core_lookup: Result<Option<RecoveryRecord>, ErrorCode> =
        update(&pic, journal, core, "recovery_event", event(1).logical_id).unwrap();
    assert!(core_lookup.unwrap().is_none());
    let stranger_read: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, stranger, "recovery_events", (0u64, 10u32)).unwrap();
    assert!(matches!(
        stranger_read,
        Err(ErrorCode::Unauthenticated { .. })
    ));
    pic.upgrade_canister(
        journal,
        pocket_ic_tests::wasm(SEND_JOURNAL_WASM),
        candid::encode_one(()).unwrap(),
        Some(controller),
    )
    .unwrap();
    let head: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "recovery_head", ()).unwrap();
    assert_eq!(head.unwrap(), second);
}

#[test]
fn only_registered_worker_can_append_and_read_its_chain() {
    let pic = pic();
    let controller = principal(201);
    let worker = principal(202);
    let stranger = principal(203);
    let journal = deploy(
        &pic,
        SEND_JOURNAL_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );

    let denied: Result<JournalHead, ErrorCode> =
        update(&pic, journal, stranger, "append", intent(1)).unwrap();
    assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
    let registered: Result<(), ErrorCode> = update_args(
        &pic,
        journal,
        controller,
        "register_worker",
        ("core".to_string(), worker),
    )
    .unwrap();
    registered.unwrap();
    let wrong_controller: Result<(), ErrorCode> = update_args(
        &pic,
        journal,
        stranger,
        "register_worker",
        ("vault".to_string(), stranger),
    )
    .unwrap();
    assert!(matches!(
        wrong_controller,
        Err(ErrorCode::Unauthenticated { .. })
    ));

    let first: Result<JournalHead, ErrorCode> =
        update(&pic, journal, worker, "append", intent(1)).unwrap();
    let first = first.unwrap();
    assert_eq!(first.sequence, 1);
    let repeated: Result<JournalHead, ErrorCode> =
        update(&pic, journal, worker, "append", intent(1)).unwrap();
    assert_eq!(repeated.unwrap(), first);
    let conflicting: Result<JournalHead, ErrorCode> =
        update(&pic, journal, worker, "append", intent(2)).unwrap();
    assert!(matches!(conflicting, Err(ErrorCode::ReservationConflict)));

    let records: Result<Vec<JournalRecord>, ErrorCode> =
        update_args(&pic, journal, worker, "records", (0u64, 10u32)).unwrap();
    let records = records.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].sequence, 1);
    assert_eq!(records[0].previous_hash.as_ref(), &[0; 32]);
    assert_eq!(records[0].hash, first.hash);
    let lookup: Result<Option<JournalRecord>, ErrorCode> = update_args(
        &pic,
        journal,
        worker,
        "intent_record",
        ("order".to_string(), intent(1).request_id),
    )
    .unwrap();
    assert_eq!(lookup.unwrap().unwrap(), records[0]);
    let denied_lookup: Result<Option<JournalRecord>, ErrorCode> = update_args(
        &pic,
        journal,
        stranger,
        "intent_record",
        ("order".to_string(), intent(1).request_id),
    )
    .unwrap();
    assert!(matches!(
        denied_lookup,
        Err(ErrorCode::Unauthenticated { .. })
    ));
    let denied: Result<Vec<JournalRecord>, ErrorCode> =
        update_args(&pic, journal, stranger, "records", (0u64, 10u32)).unwrap();
    assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
    let unbounded: Result<Vec<JournalRecord>, ErrorCode> =
        update_args(&pic, journal, worker, "records", (0u64, 101u32)).unwrap();
    assert!(matches!(unbounded, Err(ErrorCode::BadRequest { .. })));
    let head: Result<JournalHead, ErrorCode> = update(&pic, journal, worker, "head", ()).unwrap();
    assert_eq!(head.unwrap(), first);
}

#[test]
fn post_upgrade_resume_requires_sns_guard_and_matching_journal() {
    let pic = pic();
    let sns = principal(211);
    let stranger = principal(212);
    let guard = deploy(
        &pic,
        CONTROL_GUARD_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let configured: Result<(), ErrorCode> =
        update(&pic, vault, sns, "set_journal_guard", guard).unwrap();
    configured.unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, guard, sns, "set_sns_principal", sns).unwrap();
    configured.unwrap();
    pic.upgrade_canister(
        vault,
        pocket_ic_tests::wasm(FUNDS_VAULT_WASM),
        candid::encode_one(()).unwrap(),
        Some(sns),
    )
    .expect("upgrade vault");
    let denied: Result<(), ErrorCode> =
        update(&pic, vault, stranger, "resume_journal", ()).unwrap();
    assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
    let denied: Result<(), ErrorCode> =
        update(&pic, guard, stranger, "resume_journal", vault).unwrap();
    assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
    let resumed: Result<(), ErrorCode> = update(&pic, guard, sns, "resume_journal", vault).unwrap();
    resumed.unwrap();
}

#[test]
fn both_missing_remote_streams_are_staged_but_cannot_resume_sends() {
    let pic = pic();
    let sns = principal(221);
    let guard = deploy(
        &pic,
        CONTROL_GUARD_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let journal: Result<Option<candid::Principal>, ErrorCode> =
        pocket_ic_tests::query(&pic, vault, sns, "get_send_journal", ()).unwrap();
    let journal = journal.unwrap().expect("configured journal");
    let configured: Result<(), ErrorCode> =
        update(&pic, guard, sns, "set_sns_principal", sns).unwrap();
    configured.unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, vault, sns, "set_journal_guard", guard).unwrap();
    configured.unwrap();
    // 古いbackupには存在しない送信意図を独立journalへ追記する。
    let appended: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append", intent(7)).unwrap();
    assert_eq!(appended.unwrap().sequence, 1);
    let mut second = intent(8);
    second.request_id = vec![8; 32].into();
    let appended: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append", second).unwrap();
    assert_eq!(appended.unwrap().sequence, 2);
    let business: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append_recovery_event", event(7)).unwrap();
    assert_eq!(business.unwrap().sequence, 1);
    pic.upgrade_canister(
        vault,
        pocket_ic_tests::wasm(FUNDS_VAULT_WASM),
        candid::encode_one(()).unwrap(),
        Some(sns),
    )
    .unwrap();
    let resumed: Result<(), ErrorCode> = update(&pic, guard, sns, "resume_journal", vault).unwrap();
    assert!(matches!(resumed, Err(ErrorCode::PolicyUnavailable)));
    let status: Result<(u64, u64, bool), ErrorCode> =
        pocket_ic_tests::query(&pic, vault, sns, "journal_restore_status", ()).unwrap();
    assert_eq!(status.unwrap(), (0, 2, true));
    let business_status: Result<(u64, bool), ErrorCode> =
        pocket_ic_tests::query(&pic, vault, sns, "recovery_stage_status", ()).unwrap();
    assert_eq!(business_status.unwrap(), (1, true));
    let resumed: Result<(), ErrorCode> = update(&pic, guard, sns, "resume_journal", vault).unwrap();
    assert!(matches!(resumed, Err(ErrorCode::PolicyUnavailable)));
}

#[test]
fn v1_matching_backup_cannot_resume_when_business_stream_has_unreplayed_event() {
    let pic = pic();
    let sns = principal(241);
    let guard = deploy(
        &pic,
        CONTROL_GUARD_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let journal: Result<Option<candid::Principal>, ErrorCode> =
        pocket_ic_tests::query(&pic, vault, sns, "get_send_journal", ()).unwrap();
    let journal = journal.unwrap().unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, guard, sns, "set_sns_principal", sns).unwrap();
    configured.unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, vault, sns, "set_journal_guard", guard).unwrap();
    configured.unwrap();
    let empty_journal = pic
        .take_canister_snapshot(journal, Some(sns), None)
        .expect("snapshot empty independent journal");
    let appended: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append_recovery_event", event(9)).unwrap();
    assert_eq!(appended.unwrap().sequence, 1);
    pic.upgrade_canister(
        vault,
        pocket_ic_tests::wasm(FUNDS_VAULT_WASM),
        candid::encode_one(()).unwrap(),
        Some(sns),
    )
    .unwrap();
    let resumed: Result<(), ErrorCode> = update(&pic, guard, sns, "resume_journal", vault).unwrap();
    assert!(matches!(resumed, Err(ErrorCode::PolicyUnavailable)));
    let staged: Result<(u64, bool), ErrorCode> =
        pocket_ic_tests::query(&pic, vault, sns, "recovery_stage_status", ()).unwrap();
    assert_eq!(staged.unwrap(), (1, true));
    let hidden: Result<(u64, bool), ErrorCode> =
        pocket_ic_tests::query(&pic, vault, principal(242), "recovery_stage_status", ()).unwrap();
    assert!(matches!(hidden, Err(ErrorCode::Unauthenticated { .. })));
    pic.load_canister_snapshot(journal, Some(sns), empty_journal.id)
        .expect("restore independent journal behind staged worker");
    let resumed: Result<(), ErrorCode> = update(&pic, guard, sns, "resume_journal", vault).unwrap();
    assert!(matches!(resumed, Err(ErrorCode::PolicyUnavailable)));
    let staged: Result<(u64, bool), ErrorCode> =
        pocket_ic_tests::query(&pic, vault, sns, "recovery_stage_status", ()).unwrap();
    assert_eq!(staged.unwrap(), (1, true));
}

#[test]
fn prepared_send_authorization_and_cancellation_are_mutually_exclusive() {
    let pic = pic();
    let controller = principal(211);
    let worker = principal(212);
    let stranger = principal(213);
    let journal = deploy(
        &pic,
        SEND_JOURNAL_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let registered: Result<(), ErrorCode> = update_args(
        &pic,
        journal,
        controller,
        "register_worker",
        ("vault".to_string(), worker),
    )
    .unwrap();
    registered.unwrap();
    for (seed, authorize_first) in [(21u8, false), (22, true)] {
        let mut prepared = intent(1);
        prepared.kind = "allocation".into();
        prepared.request_id = vec![seed; 32].into();
        let head: Result<JournalHead, ErrorCode> =
            update(&pic, journal, worker, "append_prepared", prepared.clone()).unwrap();
        head.unwrap();
        let key = (prepared.kind.clone(), prepared.request_id.clone());
        let denied: Result<bool, ErrorCode> =
            update_args(&pic, journal, stranger, "cancel_prepared_send", key.clone()).unwrap();
        assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
        if authorize_first {
            let granted: Result<bool, ErrorCode> =
                update_args(&pic, journal, worker, "authorize_send", key.clone()).unwrap();
            assert!(granted.unwrap());
        }
        for _ in 0..2 {
            let cancelled: Result<bool, ErrorCode> =
                update_args(&pic, journal, worker, "cancel_prepared_send", key.clone()).unwrap();
            assert_eq!(cancelled.unwrap(), !authorize_first);
            let granted: Result<bool, ErrorCode> =
                update_args(&pic, journal, worker, "authorize_send", key.clone()).unwrap();
            assert!(
                !granted.unwrap(),
                "late/repeated authorization must never grant a send"
            );
        }
        let reopened: Result<JournalHead, ErrorCode> =
            update(&pic, journal, worker, "append_prepared", prepared).unwrap();
        assert!(
            reopened.is_err(),
            "terminal permission cannot be prepared again"
        );
    }
    // An old-format intent is not evidence that a POST was prevented.
    let mut legacy = intent(2);
    legacy.kind = "allocation".into();
    let head: Result<JournalHead, ErrorCode> =
        update(&pic, journal, worker, "append", legacy.clone()).unwrap();
    head.unwrap();
    let cancelled: Result<bool, ErrorCode> = update_args(
        &pic,
        journal,
        worker,
        "cancel_prepared_send",
        (legacy.kind.clone(), legacy.request_id.clone()),
    )
    .unwrap();
    assert!(!cancelled.unwrap());
    let upgraded: Result<JournalHead, ErrorCode> =
        update(&pic, journal, worker, "append_prepared", legacy).unwrap();
    assert!(
        upgraded.is_err(),
        "legacy intent cannot acquire a new unsent certificate"
    );
}
