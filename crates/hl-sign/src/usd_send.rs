//! Pure construction of the user-signed `usdSend` digest and JSON body.
//! Vault dispatch and journal replay use this same encoding.

use crate::{Signature, user_signed};
use hl_types::Network;

pub struct UsdSend<'a> {
    pub destination: &'a str,
    pub amount_micros: u64,
    pub time: u64,
}

pub fn amount_text(micros: u64) -> String {
    hl_types::UsdcMicros::from_micros(micros).to_decimal_string()
}

impl UsdSend<'_> {
    pub fn digest(&self, network: Network) -> Result<[u8; 32], String> {
        let chain_name = hl_types::environment::chain_name(network)
            .map_err(|error| format!("usdSend chain: {error:?}"))?;
        let chain_id = hl_types::environment::user_signed_chain_id(network)
            .map_err(|error| format!("usdSend chain id: {error:?}"))?;
        user_signed::digest(
            chain_id,
            user_signed::USD_SEND_PRIMARY_TYPE,
            user_signed::USD_SEND_FIELDS,
            &[
                user_signed::TypedValue::String(chain_name.to_string()),
                user_signed::TypedValue::String(self.destination.to_string()),
                user_signed::TypedValue::String(amount_text(self.amount_micros)),
                user_signed::TypedValue::Uint64(self.time),
            ],
        )
        .map_err(|error| format!("usdSend digest: {error}"))
    }

    pub fn body(&self, network: Network, signature: &Signature) -> Result<Vec<u8>, String> {
        let chain_name = hl_types::environment::chain_name(network)
            .map_err(|error| format!("usdSend chain: {error:?}"))?;
        let signature_chain_id = hl_types::environment::signature_chain_id(network)
            .map_err(|error| format!("usdSend signature chain: {error:?}"))?;
        let body = serde_json::json!({
            "action": {
                "type": "usdSend",
                "signatureChainId": signature_chain_id,
                "hyperliquidChain": chain_name,
                "destination": self.destination,
                "amount": amount_text(self.amount_micros),
                "time": self.time,
            },
            "nonce": self.time,
            "signature": {
                "r": format!("0x{}", hex::encode(signature.r)),
                "s": format!("0x{}", hex::encode(signature.s)),
                "v": signature.v,
            },
        });
        serde_json::to_vec(&body).map_err(|error| format!("usdSend body: {error}"))
    }
}
