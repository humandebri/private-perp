//! canister側のHPKE封筒（暗号化・復号と`aad`束縛）の試験（2D）。

use api_types::error::ErrorCode;
use candid::Principal;
use pocket_ic_tests::{FUNDS_VAULT_WASM, deploy, pic, principal, query_args, update, update_args};

fn deploy_vault(pic: &pocket_ic::PocketIc, controller: Principal) -> Principal {
    deploy(
        pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    )
}

#[test]
fn the_canister_seals_and_opens_with_bound_aad() {
    let pic = pic();
    let controller = principal(130);
    let vault = deploy_vault(&pic, controller);

    let rotated: Result<Vec<u8>, ErrorCode> =
        update(&pic, vault, controller, "rotate_hpke_key", ()).expect("call");
    rotated.expect("rotate");

    let caller = principal(131);
    let plaintext = b"{\"market\":\"BTC\",\"quantity\":\"0.01\"}".to_vec();
    let sealed: Result<Vec<u8>, ErrorCode> =
        update(&pic, vault, caller, "test_hpke_seal", plaintext.clone()).expect("call");
    let envelope = sealed.expect("seal");
    let envelope_before = envelope.clone();
    assert!(envelope.len() > 32, "封筒はencと暗号文を含む");

    // 正しいaad（同じ呼び出し元・同じ期限）では復号できる。
    let opened: Result<Vec<u8>, ErrorCode> = query_args(
        &pic,
        vault,
        caller,
        "test_hpke_open",
        (envelope.clone(), 0u64),
    )
    .expect("call");
    assert_eq!(opened.expect("open"), plaintext);

    // 期限が違うとaadが変わり復号に失敗する。
    let wrong_expiry: Result<Vec<u8>, ErrorCode> = query_args(
        &pic,
        vault,
        caller,
        "test_hpke_open",
        (envelope.clone(), 1u64),
    )
    .expect("call");
    assert!(wrong_expiry.is_err(), "aad不一致は失敗する");

    // 呼び出し元が違えばaadが変わり復号に失敗する。
    let other: Result<Vec<u8>, ErrorCode> = query_args(
        &pic,
        vault,
        principal(132),
        "test_hpke_open",
        (envelope, 0u64),
    )
    .expect("call");
    assert!(other.is_err(), "caller不一致は失敗する");

    // 封筒は呼び出しごとに異なる（乱数が効いている）。
    let sealed_again: Result<Vec<u8>, ErrorCode> =
        update_args(&pic, vault, caller, "test_hpke_seal", (plaintext,)).expect("call");
    assert_ne!(
        sealed_again.expect("seal"),
        envelope_before,
        "乱数により封筒は毎回異なる"
    );
}
