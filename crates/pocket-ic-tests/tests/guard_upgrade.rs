//! `control_guard` の7日猶予と迂回拒否の試験（`docs/phase-0/threat-test-matrix.md` T-501〜T-506）。

use api_types::error::{ErrorCode, NotAllowedCode};
use api_types::guard::{ScheduleUpgradeArgs, UpgradeRequest, UpgradeState, UpgradeStatus};
use candid::Principal;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    CONTROL_GUARD_WASM, POLICY_WASM, deploy, pic, principal, query, update, update_args, wasm,
};
use std::time::Duration;

const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// 極小の有効なwasmモジュール（ヘッダのみ）。`install_code`へ渡すサイズ上限を避けて
/// 実行経路を検証するために使う。
const MINIMAL_WASM: &[u8] = b"\0asm\x01\0\0\0";

fn deploy_guard(pic: &PocketIc, controller: Principal) -> Principal {
    deploy(
        pic,
        CONTROL_GUARD_WASM,
        Some(vec![controller]),
        candid::encode_one(()).expect("encode ()"),
    )
}

fn schedule(
    pic: &PocketIc,
    guard: Principal,
    caller: Principal,
    target: Principal,
    wasm_hash: [u8; 32],
) -> Result<(), ErrorCode> {
    let args = ScheduleUpgradeArgs {
        request: UpgradeRequest {
            target,
            wasm_hash: wasm_hash.to_vec().into(),
            // 実行時に渡す引数（空）と同じhashを予約する。
            arg_hash: hash_of(&[]).to_vec().into(),
        },
    };
    update(pic, guard, caller, "schedule_upgrade", args).expect("schedule call")
}

fn hash_of(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn execute(
    pic: &PocketIc,
    guard: Principal,
    caller: Principal,
    target: Principal,
    wasm_module: Vec<u8>,
) -> Result<(), ErrorCode> {
    update_args(
        pic,
        guard,
        caller,
        "execute_upgrade",
        (target, wasm_module, Vec::<u8>::new()),
    )
    .expect("execute call")
}

fn status(pic: &PocketIc, guard: Principal) -> UpgradeStatus {
    query::<(), UpgradeStatus>(pic, guard, Principal::anonymous(), "get_upgrade_status", ())
        .expect("status call")
}

#[test]
fn only_the_sns_principal_can_schedule() {
    let pic = pic();
    let controller = principal(50);
    let guard = deploy_guard(&pic, controller);
    let sns = principal(51);
    let target = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![guard]),
        candid::encode_one(()).unwrap(),
    );
    let wasm_hash = hash_of(&wasm(POLICY_WASM));

    // SNS principal未設定では予約できない。
    let unset = schedule(&pic, guard, sns, target, wasm_hash).expect_err("must reject");
    assert!(matches!(unset, ErrorCode::Internal { .. }), "{unset:?}");

    // controllerがSNS principalを設定できる。
    let set: Result<(), ErrorCode> =
        update(&pic, guard, controller, "set_sns_principal", sns).expect("call");
    set.expect("set_sns_principal");

    // 非SNSからの予約は拒否する（T-501）。
    let denied = schedule(&pic, guard, principal(52), target, wasm_hash).expect_err("must reject");
    assert!(
        matches!(denied, ErrorCode::Unauthenticated { .. }),
        "{denied:?}"
    );

    // SNSからの予約は受理され、7日後が実行可能時刻になる。
    schedule(&pic, guard, sns, target, wasm_hash).expect("schedule");
    let scheduled = status(&pic, guard).scheduled.expect("scheduled");
    assert_eq!(scheduled.executable_at - scheduled.scheduled_at, 7 * DAY_MS);
    assert_eq!(scheduled.state, UpgradeState::Pending, "猶予前はpending");
}

#[test]
fn execution_is_blocked_until_seven_days_and_on_content_mismatch() {
    let pic = pic();
    let controller = principal(53);
    let guard = deploy_guard(&pic, controller);
    let sns = principal(54);
    let target = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![guard]),
        candid::encode_one(()).unwrap(),
    );
    let wasm_hash = hash_of(&wasm(POLICY_WASM));

    let set: Result<(), ErrorCode> =
        update(&pic, guard, controller, "set_sns_principal", sns).expect("call");
    set.expect("set_sns_principal");
    schedule(&pic, guard, sns, target, wasm_hash).expect("schedule");

    // 7日未満の実行は拒否する（T-502）。
    let early = execute(&pic, guard, principal(55), target, vec![0u8; 64]).expect_err("early");
    assert_eq!(
        early,
        ErrorCode::NotAllowed {
            code: NotAllowedCode::UpgradeTooEarly
        }
    );

    pic.advance_time(Duration::from_millis(7 * DAY_MS + DAY_MS));
    for _ in 0..5 {
        pic.tick();
    }

    // 内容が一致しないupgradeは拒否する（T-503/T-504）。不一致は install_code の前に
    // 判定されるため、小さなダミーで検証できる。
    let mismatched =
        execute(&pic, guard, principal(55), target, vec![0u8; 64]).expect_err("mismatch");
    assert_eq!(
        mismatched,
        ErrorCode::NotAllowed {
            code: NotAllowedCode::UpgradeContentMismatch
        }
    );

    // 予約は保持されたまま（一致する実行は下の wasm サイズ制約のため保留）。
    let scheduled = status(&pic, guard).scheduled.expect("scheduled");
    assert!(
        scheduled.state == UpgradeState::Executable,
        "猶予の経過後はexecutableとして返す（保存状態は書き換えない）"
    );
}

/// 一致する予約は実行できる（極小wasmで`install_code`まで検証する）。
#[test]
fn a_matching_reservation_executes_the_upgrade() {
    let pic = pic();
    let controller = principal(53);
    let guard = deploy_guard(&pic, controller);
    let sns = principal(54);
    let target = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![guard]),
        candid::encode_one(()).unwrap(),
    );
    let wasm_hash = hash_of(MINIMAL_WASM);

    let set: Result<(), ErrorCode> =
        update(&pic, guard, controller, "set_sns_principal", sns).expect("call");
    set.expect("set_sns_principal");
    schedule(&pic, guard, sns, target, wasm_hash).expect("schedule");
    pic.advance_time(Duration::from_millis(7 * DAY_MS + DAY_MS));
    for _ in 0..5 {
        pic.tick();
    }

    execute(&pic, guard, principal(55), target, MINIMAL_WASM.to_vec()).expect("execute");
    assert!(
        status(&pic, guard).scheduled.is_none(),
        "実行後は予約が消える"
    );

    // 二重実行は拒否する。
    let again =
        execute(&pic, guard, principal(55), target, MINIMAL_WASM.to_vec()).expect_err("again");
    assert_eq!(
        again,
        ErrorCode::NotAllowed {
            code: NotAllowedCode::UpgradeNotScheduled
        }
    );
}

/// 実行権は単一の実行者だけが取れる（同時投入でも二重インストールしない）。
#[test]
fn only_one_concurrent_execution_wins() {
    let pic = pic();
    let controller = principal(53);
    let guard = deploy_guard(&pic, controller);
    let sns = principal(54);
    let target = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![guard]),
        candid::encode_one(()).unwrap(),
    );
    let wasm_hash = hash_of(MINIMAL_WASM);

    let set: Result<(), ErrorCode> =
        update(&pic, guard, controller, "set_sns_principal", sns).expect("call");
    set.expect("set_sns_principal");
    schedule(&pic, guard, sns, target, wasm_hash).expect("schedule");
    pic.advance_time(Duration::from_millis(7 * DAY_MS + DAY_MS));
    for _ in 0..5 {
        pic.tick();
    }

    let payload =
        candid::encode_args((target, MINIMAL_WASM.to_vec(), Vec::<u8>::new())).expect("encode");
    let first = pic
        .submit_call(guard, principal(55), "execute_upgrade", payload.clone())
        .expect("submit 1");
    let second = pic
        .submit_call(guard, principal(56), "execute_upgrade", payload)
        .expect("submit 2");

    let outcomes: Vec<Result<(), ErrorCode>> = [first, second]
        .into_iter()
        .map(|id| {
            let bytes = pic.await_call(id).expect("await");
            candid::decode_one::<Result<(), ErrorCode>>(&bytes).expect("decode")
        })
        .collect();

    let wins = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
    let losses = outcomes
        .iter()
        .filter(|outcome| {
            matches!(
                outcome,
                Err(ErrorCode::NotAllowed {
                    code: NotAllowedCode::UpgradeAlreadyExecuted
                }) | Err(ErrorCode::NotAllowed {
                    code: NotAllowedCode::UpgradeNotScheduled
                })
            )
        })
        .count();
    assert_eq!(wins, 1, "実行者は1つだけ: {outcomes:?}");
    assert_eq!(losses, 1, "他方は拒否される: {outcomes:?}");
    assert!(status(&pic, guard).scheduled.is_none());
}

#[test]
fn cancelling_starts_a_new_seven_day_window() {
    let pic = pic();
    let controller = principal(56);
    let guard = deploy_guard(&pic, controller);
    let sns = principal(57);
    let target = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![guard]),
        candid::encode_one(()).unwrap(),
    );
    let wasm_hash = hash_of(&wasm(POLICY_WASM));

    let set: Result<(), ErrorCode> =
        update(&pic, guard, controller, "set_sns_principal", sns).expect("call");
    set.expect("set_sns_principal");
    schedule(&pic, guard, sns, target, wasm_hash).expect("schedule");

    pic.advance_time(Duration::from_millis(3 * DAY_MS));
    pic.tick();

    let cancelled: Result<(), ErrorCode> =
        update(&pic, guard, sns, "cancel_upgrade", ()).expect("call");
    cancelled.expect("cancel_upgrade");
    assert!(status(&pic, guard).scheduled.is_none());

    // 予約内容の変更は取消＋新規予約であり、新しい7日を開始する。
    schedule(&pic, guard, sns, target, wasm_hash).expect("reschedule");
    let rescheduled = status(&pic, guard).scheduled.expect("scheduled");
    assert_eq!(
        rescheduled.executable_at - rescheduled.scheduled_at,
        7 * DAY_MS
    );

    // 取消前の猶予を引き継がない（新しい予約は実行可能になっていない）。
    let early = execute(&pic, guard, principal(58), target, wasm(POLICY_WASM)).expect_err("early");
    assert_eq!(
        early,
        ErrorCode::NotAllowed {
            code: NotAllowedCode::UpgradeTooEarly
        }
    );

    // 非SNSによる取消は拒否する。
    let denied: Result<(), ErrorCode> =
        update(&pic, guard, principal(59), "cancel_upgrade", ()).expect("call");
    let denied = denied.expect_err("non-SNS cancel must be rejected");
    assert!(matches!(denied, ErrorCode::Unauthenticated { .. }));
}
