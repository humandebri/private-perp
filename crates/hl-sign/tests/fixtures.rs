//! 公式SDKのテストベクトルとの比較（`Implementation.md` 9.1）。
//!
//! `crates/hl-sign/tests/fixtures/*.json` は `tools/hl-fixture-gen/` が
//! `@nktkas/hyperliquid` から生成したfixtureである。署名方式は2系統ある。
//!
//! - L1（phantom agent）: order / cancel / cancelByCloid / updateLeverage など。
//!   `connection_id_hex` にSDKの `createL1ActionHash` の値が入る。
//! - user-signed EIP-712: approveAgent / usdSend など。
//!   `connection_id_hex` は `null` で、`action` の型定義からダイジェストを計算する。
//!
//! どちらも次を確認する。
//!
//! 1. 秘密鍵から導いたアドレスがfixtureの `address` と一致する。
//! 2. 計算したactionハッシュ（L1のみ）がSDKの値と一致する。
//! 3. 決定的に署名した結果がSDKの署名と一致する（r/s/v）。
//! 4. その署名から復元したアドレスが署名者と一致する。
//!
//! SDKは中間値（msgpack・digest）を公開していないため、fixtureには入れない。
//! 3の署名一致がdigest一致の強い証拠になる。

use hl_sign::user_signed::{TypedField, TypedValue};
use hl_sign::{
    ActionHashInput, Signature, action_hash, address_from_secret, recover_address, sign_action,
    signing_digest, user_signed,
};
use hl_types::msgpack::Value;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct FixtureSignature {
    r: String,
    s: String,
    v: u8,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    network: String,
    private_key_hex: String,
    address: String,
    is_agent: bool,
    vault_address: Option<String>,
    expires_after: Option<u64>,
    nonce: u64,
    action: serde_json::Value,
    /// SDKの `createL1ActionHash` の結果。user-signedでは `null`。
    connection_id_hex: Option<String>,
    signature_hex: Option<String>,
    signature: Option<FixtureSignature>,
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_fixtures() -> Vec<(PathBuf, Fixture)> {
    let dir = fixtures_dir();
    let mut fixtures = Vec::new();
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|error| {
        panic!(
            "{} を読めません（{error}）。tools/hl-fixture-gen でfixtureを生成してください",
            dir.display()
        )
    });
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let bytes = std::fs::read(&path).expect("read fixture");
        let fixture: Fixture = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("{} を解析できません: {error}", path.display()));
        fixtures.push((path, fixture));
    }
    assert!(
        !fixtures.is_empty(),
        "{} にfixtureがありません。tools/hl-fixture-gen で生成してください",
        dir.display()
    );
    fixtures
}

/// `0x` 付きhexを固定長バイト列へ。
fn hex_bytes<const N: usize>(value: &str, what: &str) -> [u8; N] {
    let trimmed = value.strip_prefix("0x").unwrap_or(value);
    let bytes = (0..trimmed.len() / 2)
        .map(|index| u8::from_str_radix(&trimmed[index * 2..index * 2 + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .unwrap_or_else(|error| panic!("{what} をhexとして読めません: {error}"));
    bytes
        .try_into()
        .unwrap_or_else(|_| panic!("{what} の長さが{N}バイトではありません: {value}"))
}

/// JSONのオブジェクト順序を保ったままmsgpack値へ変換する。
///
/// 浮動小数点は受け付けない（`docs/phase-0/money-and-units.md` 2節）。
fn json_to_msgpack(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(flag) => Value::Bool(*flag),
        serde_json::Value::Number(number) => {
            if let Some(unsigned) = number.as_u64() {
                Value::UInt(unsigned)
            } else if let Some(signed) = number.as_i64() {
                Value::Int(signed)
            } else {
                panic!("actionに浮動小数点数が含まれています: {number}");
            }
        }
        serde_json::Value::String(text) => Value::owned(text.clone()),
        serde_json::Value::Array(items) => {
            Value::Array(items.iter().map(json_to_msgpack).collect())
        }
        serde_json::Value::Object(map) => Value::Map(
            map.iter()
                .map(|(key, value)| (Value::owned(key.clone()), json_to_msgpack(value)))
                .collect(),
        ),
    }
}

fn expected_signature(fixture: &Fixture) -> Signature {
    if let Some(signature) = &fixture.signature {
        return Signature {
            r: hex_bytes::<32>(&signature.r, "signature.r"),
            s: hex_bytes::<32>(&signature.s, "signature.s"),
            v: match signature.v {
                0 | 27 => 27,
                1 | 28 => 28,
                other => panic!("未知の v: {other}"),
            },
        };
    }
    let raw = fixture
        .signature_hex
        .as_deref()
        .expect("signature_hex も signature もありません");
    let bytes: [u8; 65] = hex_bytes::<65>(raw, "signature_hex");
    Signature::from_bytes65(&bytes).expect("65バイトの署名")
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|entry| entry.as_str())
        .unwrap_or_else(|| panic!("action.{key} が文字列ではありません"))
        .to_string()
}

fn json_u64(value: &serde_json::Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(|entry| entry.as_u64())
        .unwrap_or_else(|| panic!("action.{key} がu64ではありません"))
}

/// user-signed actionの型・値へ変換する。
fn user_signed_fields(
    action: &serde_json::Value,
) -> (&'static str, &'static [TypedField], Vec<TypedValue>) {
    let action_type = action
        .get("type")
        .and_then(|entry| entry.as_str())
        .expect("action.type");
    match action_type {
        "approveAgent" => {
            let agent_name = action
                .get("agentName")
                .and_then(|entry| entry.as_str())
                .unwrap_or("")
                .to_string();
            (
                user_signed::APPROVE_AGENT_PRIMARY_TYPE,
                user_signed::APPROVE_AGENT_FIELDS,
                vec![
                    TypedValue::String(json_string(action, "hyperliquidChain")),
                    TypedValue::Address(hex_bytes::<20>(
                        &json_string(action, "agentAddress"),
                        "agentAddress",
                    )),
                    TypedValue::String(agent_name),
                    TypedValue::Uint64(json_u64(action, "nonce")),
                ],
            )
        }
        "usdSend" => (
            user_signed::USD_SEND_PRIMARY_TYPE,
            user_signed::USD_SEND_FIELDS,
            vec![
                TypedValue::String(json_string(action, "hyperliquidChain")),
                TypedValue::String(json_string(action, "destination")),
                TypedValue::String(json_string(action, "amount")),
                TypedValue::Uint64(json_u64(action, "time")),
            ],
        ),
        other => panic!("未対応のuser-signed action: {other}"),
    }
}

#[test]
fn fixtures_match_the_official_sdk() {
    let fixtures = load_fixtures();
    assert!(
        fixtures.len() >= 8,
        "fixtureが8件未満です（{}件）",
        fixtures.len()
    );

    let mut l1_count = 0;
    let mut user_signed_count = 0;

    for (path, fixture) in &fixtures {
        let label = format!("{} ({})", fixture.name, path.display());
        let secret_key: [u8; 32] = hex_bytes(&fixture.private_key_hex, "private_key_hex");

        // 1. 署名者のアドレス。
        let address = address_from_secret(&secret_key).expect("address");
        let expected_address: [u8; 20] = hex_bytes(&fixture.address, "address");
        assert_eq!(address, expected_address, "{label}: アドレスが一致しません");

        let expected = expected_signature(fixture);

        if let Some(connection_id) = fixture.connection_id_hex.as_deref() {
            // L1（phantom agent）経路。
            l1_count += 1;
            let action_msgpack = json_to_msgpack(&fixture.action).encode();
            let vault_address = fixture
                .vault_address
                .as_deref()
                .map(|value| hex_bytes::<20>(value, "vault_address"));
            let input = ActionHashInput {
                action_msgpack: &action_msgpack,
                nonce: fixture.nonce,
                vault_address,
                expires_after: fixture.expires_after,
            };
            let hash = action_hash(&input);
            let expected_hash: [u8; 32] = hex_bytes::<32>(connection_id, "connection_id_hex");
            assert_eq!(
                hash, expected_hash,
                "{label}: actionハッシュが公式SDKと一致しません"
            );

            if fixture.is_agent {
                assert_eq!(
                    connection_id.len(),
                    66,
                    "{label}: agentのconnectionIdは32バイトです"
                );
            }

            let mainnet = fixture.network == "mainnet";
            let signature = sign_action(&input, &secret_key, mainnet).expect("sign");
            assert_eq!(signature, expected, "{label}: 署名が公式SDKと一致しません");

            let digest = signing_digest(hash, mainnet);
            assert_eq!(
                recover_address(&digest, &signature, None).expect("recover"),
                expected_address,
                "{label}: 復元アドレスが一致しません"
            );
        } else {
            // user-signed EIP-712経路。
            user_signed_count += 1;
            let chain_id_hex = json_string(&fixture.action, "signatureChainId");
            let chain_id = u64::from_str_radix(chain_id_hex.trim_start_matches("0x"), 16)
                .expect("signatureChainId");
            let (primary_type, fields, values) = user_signed_fields(&fixture.action);

            let digest = user_signed::digest(chain_id, primary_type, fields, &values)
                .expect("user-signed digest");
            let signature = user_signed::sign(chain_id, primary_type, fields, &values, &secret_key)
                .expect("user-signed sign");
            assert_eq!(
                signature, expected,
                "{label}: user-signed署名が公式SDKと一致しません"
            );
            assert_eq!(
                recover_address(&digest, &signature, None).expect("recover"),
                expected_address,
                "{label}: 復元アドレスが一致しません"
            );
        }
    }

    assert!(l1_count >= 5, "L1経路のfixtureが不足しています");
    assert!(
        user_signed_count >= 2,
        "user-signed経路のfixtureが不足しています"
    );
}
