//! LOCAL MOCK helper: derive the real sender from the signed usdSend payload.
use hl_sign::{Signature, recover_address, user_signed};
use serde_json::Value;
use std::io::{self, Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let body: Value = serde_json::from_str(&input)?;
    let action = &body["action"];
    if action["type"] != "usdSend" || action["time"] != body["nonce"] {
        return Err("invalid usdSend".into());
    }
    let text = |field: &str| action[field].as_str().ok_or("missing action field");
    let chain = text("hyperliquidChain")?;
    if chain != "Testnet" {
        return Err("mock only accepts Testnet signatures".into());
    }
    let chain_id = u64::from_str_radix(text("signatureChainId")?.trim_start_matches("0x"), 16)?;
    let digest = user_signed::digest(
        chain_id,
        user_signed::USD_SEND_PRIMARY_TYPE,
        user_signed::USD_SEND_FIELDS,
        &[
            user_signed::TypedValue::String(chain.to_string()),
            user_signed::TypedValue::String(text("destination")?.to_string()),
            user_signed::TypedValue::String(text("amount")?.to_string()),
            user_signed::TypedValue::Uint64(action["time"].as_u64().ok_or("missing time")?),
        ],
    )?;
    let scalar = |name: &str| -> Result<[u8; 32], Box<dyn std::error::Error>> {
        let value = body["signature"][name]
            .as_str()
            .ok_or("missing signature")?;
        Ok(hex::decode(value.trim_start_matches("0x"))?
            .try_into()
            .map_err(|_| "invalid scalar")?)
    };
    let signature = Signature {
        r: scalar("r")?,
        s: scalar("s")?,
        v: u8::try_from(body["signature"]["v"].as_u64().ok_or("missing v")?)?,
    };
    println!(
        "0x{}",
        hex::encode(recover_address(&digest, &signature, None)?)
    );
    Ok(())
}
