//! HPKE鍵世代の試験（公開鍵のみ配布、更新で世代が進む）。

use api_types::error::ErrorCode;
use candid::Principal;
use pocket_ic_tests::{FUNDS_VAULT_WASM, deploy, pic, principal, query, update};

fn deploy_vault(pic: &pocket_ic::PocketIc, controller: Principal) -> Principal {
    deploy(
        pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    )
}

#[test]
fn hpke_keys_rotate_and_only_the_public_key_is_served() {
    let pic = pic();
    let controller = principal(120);
    let vault = deploy_vault(&pic, controller);

    // 未生成では公開鍵を返さない（機密性の前提が欠けているためfail-closed）。
    let unset: Result<Vec<u8>, ErrorCode> =
        query(&pic, vault, principal(121), "get_hpke_public_key", ()).expect("call");
    assert!(unset.is_err(), "未生成はエラー: {unset:?}");

    // controllerが生成する。
    let first: Result<Vec<u8>, ErrorCode> =
        update(&pic, vault, controller, "rotate_hpke_key", ()).expect("call");
    let first = first.expect("rotate");
    assert_eq!(first.len(), 32);

    // 公開鍵は配布され、同じ世代では安定する。
    let served: Result<Vec<u8>, ErrorCode> =
        query(&pic, vault, principal(121), "get_hpke_public_key", ()).expect("call");
    assert_eq!(served.expect("public key"), first);

    // 更新すると新しい世代の公開鍵になる。
    let second: Result<Vec<u8>, ErrorCode> =
        update(&pic, vault, controller, "rotate_hpke_key", ()).expect("call");
    let second = second.expect("rotate");
    assert_ne!(second, first, "世代が進む");
    let served_again: Result<Vec<u8>, ErrorCode> =
        query(&pic, vault, principal(121), "get_hpke_public_key", ()).expect("call");
    assert_eq!(served_again.expect("public key"), second);

    // 非controllerは更新できない。
    let denied: Result<Vec<u8>, ErrorCode> =
        update(&pic, vault, principal(122), "rotate_hpke_key", ()).expect("call");
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );
}
