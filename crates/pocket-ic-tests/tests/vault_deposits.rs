//! 取引所入金の記録（二重計上防止）の試験。

use api_types::Blob;
use api_types::error::ErrorCode;
use pocket_ic_tests::{FUNDS_VAULT_WASM, deploy, pic, principal, update_args};

fn blob(value: &[u8]) -> Blob {
    value.to_vec().into()
}

#[test]
fn venue_deposits_are_recorded_once() {
    let pic = pic();
    let controller = principal(170);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );

    let tx_hash = blob(&[7u8; 32]);
    let first: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "ingest_venue_deposit",
        (tx_hash.clone(), 1_000_000u64, "usdc".to_string()),
    )
    .expect("call");
    assert!(first.expect("first ingest"), "新規の入金は記録される");

    // 同じtx_hashは二重計上しない。
    let second: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "ingest_venue_deposit",
        (tx_hash.clone(), 1_000_000u64, "usdc".to_string()),
    )
    .expect("call");
    assert!(!second.expect("second ingest"), "同じtx_hashは記録しない");

    // 空のtx_hashは拒否する。
    let empty: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "ingest_venue_deposit",
        (blob(&[]), 1u64, "usdc".to_string()),
    )
    .expect("call");
    assert!(empty.is_err(), "tx_hashは必須");

    // 非controllerは取り込めない。
    let denied: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        principal(171),
        "ingest_venue_deposit",
        (blob(&[8u8; 32]), 1u64, "usdc".to_string()),
    )
    .expect("call");
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );
}
