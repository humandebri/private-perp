//! Local/testnet synthetic eligibility signer. Secret is read only from an env var.
use api_types::eligibility::EligibilityClaims;
use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key_hex = std::env::var("LOCAL_ELIGIBILITY_ISSUER_KEY")
        .map_err(|_| "LOCAL_ELIGIBILITY_ISSUER_KEY is required")?;
    let key: [u8; 32] = hex::decode(key_hex.trim_start_matches("0x"))?
        .try_into()
        .map_err(|_| "issuer key must be 32 bytes")?;
    let address = hl_sign::address_from_secret(&key)?;
    if std::env::args().nth(1).as_deref() == Some("address") {
        println!("0x{}", hex::encode(address));
        return Ok(());
    }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let encoded = hex::decode(input.trim().trim_start_matches("0x"))?;
    let claims: EligibilityClaims = candid::decode_one(&encoded)?;
    if claims.network == api_types::Network::Mainnet {
        return Err("mainnet eligibility is disabled".into());
    }
    if claims.user_id.len() != 32 || claims.account_id.len() != 32 || claims.nonce.len() != 32 {
        return Err("invalid claim lengths".into());
    }
    let canonical = candid::encode_one(&claims)?;
    // The browser can use a different Candid type table for the same claims.
    // Sign the typed Rust encoding that the vault uses when it verifies them.
    let digest = hl_sign::keccak256_concat(&[b"private-perp/eligibility/v1", &canonical]);
    let signature = hl_sign::sign_digest_for_tests(&digest, &key)?;
    println!("0x{}", hex::encode(signature.to_bytes65()));
    Ok(())
}
