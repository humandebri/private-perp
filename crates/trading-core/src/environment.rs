//! 実行時の環境設定（network・HL endpoint・tECDSA key ID）。
//!
//! `docs/phase-0/environments.md` 4節に従い、環境の判定は**ビルド定数ではなく
//! 起動時に検証可能な値**（network・endpoint・key ID）で行う。networkは
//! `set_market_context`（`core_config.network`）を出所とし、endpointは
//! `set_venue_endpoints`の設定、未設定ならnetworkから解決する。mainnetはPhase 2では
//! 拒否する（E-2）。

use crate::map_db;
use api_types::environment::EnvironmentView;
use api_types::error::{BadRequestCode, ErrorCode};
use hl_types::environment as env;

/// 現在の環境設定を解決する（トランザクション外から）。
pub fn resolved() -> Result<EnvironmentView, ErrorCode> {
    let row = db::tx::query(db::repo::core_config::environment).map_err(map_db)?;
    resolve(&row)
}

/// 現在のnetwork名（封筒の`aad`束縛や行への記録に使う）。
pub fn network_name() -> Result<String, ErrorCode> {
    Ok(env::network_name(resolved()?.network.into()).to_string())
}

/// 生の設定値に既定値を埋めて検証する。
fn resolve(row: &db::repo::core_config::EnvironmentRow) -> Result<EnvironmentView, ErrorCode> {
    let network = match row.network.as_deref() {
        Some(name) => env::parse_network(name).map_err(map_environment)?,
        None => hl_types::Network::Local,
    };
    let (default_exchange, default_info) =
        env::default_endpoints(network).map_err(map_environment)?;
    let exchange_url = row
        .exchange_url
        .clone()
        .unwrap_or_else(|| default_exchange.to_string());
    let info_url = row
        .info_url
        .clone()
        .unwrap_or_else(|| default_info.to_string());
    env::validate_endpoints(network, &exchange_url, &info_url).map_err(map_environment)?;
    let ecdsa_key_id = match row.ecdsa_key_id.as_deref() {
        Some(key_id) => key_id.to_string(),
        None => env::default_ecdsa_key_id(network)
            .map_err(map_environment)?
            .to_string(),
    };
    env::validate_key_id(&ecdsa_key_id).map_err(map_environment)?;
    Ok(EnvironmentView {
        network: network.into(),
        exchange_url,
        info_url,
        ecdsa_key_id,
    })
}

/// 環境エラーをAPIのエラーへ写す（設定不備を理由コードつきで明示する）。
pub fn map_environment(error: env::EnvironmentError) -> ErrorCode {
    use env::EnvironmentError;
    let code = match error {
        EnvironmentError::InvalidUrl | EnvironmentError::InvalidKeyId => {
            BadRequestCode::MalformedPayload
        }
        _ => BadRequestCode::NetworkMismatch,
    };
    ErrorCode::BadRequest {
        code,
        detail: format!("environment: {}", error.code()),
    }
}
