use hl_sign::private_perp::{Challenge, Withdrawal};
use hl_sign::signature::address_from_secret;
use serde_json::Value;
use std::io::Read;

const SECRET: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 42,
];

fn bytes(value: &Value) -> Vec<u8> {
    hex::decode(value.as_str().expect("hex string").trim_start_matches("0x")).expect("hex")
}

fn address(value: &Value) -> [u8; 20] {
    bytes(value).try_into().expect("20-byte address")
}

fn u64_value(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .expect("u64")
}

fn main() {
    let mut secret = SECRET;
    if std::env::args().any(|arg| arg == "--secondary") {
        secret[31] = 43;
    }
    if std::env::args().nth(1).as_deref() == Some("address") {
        println!(
            "0x{}",
            hex::encode(address_from_secret(&secret).expect("address"))
        );
        return;
    }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).expect("stdin");
    let value: Value = serde_json::from_str(&input).expect("typed data json");
    let message = &value["message"];
    let signature = match value["primaryType"].as_str().expect("primaryType") {
        "PrivatePerpChallenge" => Challenge {
            purpose: message["purpose"].as_str().expect("purpose").into(),
            eoa: address(&message["eoa"]),
            principal: bytes(&message["principal"]),
            canister: bytes(&message["canister"]),
            network: message["network"].as_str().expect("network").into(),
            origin: message["origin"].as_str().expect("origin").into(),
            nonce: bytes(&message["nonce"]).try_into().expect("32-byte nonce"),
            expires_at: u64_value(&message["expiresAt"]),
        }
        .sign_for_tests(&secret)
        .expect("sign challenge"),
        "PrivatePerpWithdrawal" => Withdrawal {
            eoa: address(&message["eoa"]),
            amount: u64_value(&message["amount"]),
            asset: message["asset"].as_str().expect("asset").into(),
            destination: message["destination"].as_str().expect("destination").into(),
            network: message["network"].as_str().expect("network").into(),
            nonce: u64_value(&message["nonce"]),
            expires_at: u64_value(&message["expiresAt"]),
            canister: bytes(&message["canister"]),
        }
        .sign_for_tests(&secret)
        .expect("sign withdrawal"),
        other => panic!("unsupported primary type: {other}"),
    };
    println!(
        "0x{}{}{:02x}",
        hex::encode(signature.r),
        hex::encode(signature.s),
        signature.v
    );
}
