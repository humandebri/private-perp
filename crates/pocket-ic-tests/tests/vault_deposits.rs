//! 取引所入金の取り込み（記録・二重計上防止・入力検証）の試験。

use api_types::Blob;
use api_types::error::ErrorCode;
use pocket_ic::PocketIc;
use pocket_ic_tests::{FUNDS_VAULT_WASM, deploy, pic, principal, update_args};

fn blob(value: &[u8]) -> Blob {
    value.to_vec().into()
}

fn credit(
    pic: &PocketIc,
    vault: candid::Principal,
    caller: candid::Principal,
    tx: &[u8],
    amount: u64,
    address: &Blob,
) -> Result<bool, ErrorCode> {
    update_args(
        pic,
        vault,
        caller,
        "credit_venue_deposit",
        (blob(tx), amount, address.clone(), "usdc".to_string()),
    )
    .expect("call")
}

#[test]
fn venue_deposits_are_recorded_once_and_reject_bad_inputs() {
    let pic = pic();
    let controller = principal(170);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );

    // 未知の宛先は記録のみ（残高は動かさない）。
    let unknown = credit(
        &pic,
        vault,
        controller,
        &[7u8; 32],
        500_000,
        &blob(&[9u8; 20]),
    );
    assert!(unknown.expect("recorded"), "新規の入金を記録する");

    // 同じtx_hashは二重計上しない。
    let again = credit(
        &pic,
        vault,
        controller,
        &[7u8; 32],
        500_000,
        &blob(&[9u8; 20]),
    );
    assert!(!again.expect("deduped"), "同じtx_hashは記録しない");

    // 空のtx_hashは拒否する。
    let empty: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "credit_venue_deposit",
        (blob(&[]), 1u64, blob(&[9u8; 20]), "usdc".to_string()),
    )
    .expect("call");
    assert!(empty.is_err(), "tx_hashは必須");

    // 非controllerは取り込めない。
    let denied = credit(
        &pic,
        vault,
        principal(172),
        &[8u8; 32],
        1,
        &blob(&[9u8; 20]),
    );
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );
}
