//! Canister APIの状態名とDB文字列の対応（ホストでも使える）。
//!
//! DBへは文字列で保存し、API境界では `api-types` の型を使う。両者の対応を
//! 1箇所にまとめ、`docs/phase-0/state-machines.md` の正準名からずれないようにする。

use api_types::fund::{ActionState, FundRequestState};
use api_types::order::OrderState;

/// 資金要求の状態 → DB文字列。
pub fn fund_request_state_str(state: FundRequestState) -> &'static str {
    match state {
        FundRequestState::Accepted => "accepted",
        FundRequestState::Reserved => "reserved",
        FundRequestState::Executing => "executing",
        FundRequestState::Settled => "settled",
        FundRequestState::Rejected => "rejected",
        FundRequestState::Unknown => "unknown",
    }
}

/// DB文字列 → 資金要求の状態。
pub fn fund_request_state_from_str(value: &str) -> Option<FundRequestState> {
    Some(match value {
        "accepted" => FundRequestState::Accepted,
        "reserved" => FundRequestState::Reserved,
        "executing" => FundRequestState::Executing,
        "settled" => FundRequestState::Settled,
        "rejected" => FundRequestState::Rejected,
        "unknown" => FundRequestState::Unknown,
        _ => return None,
    })
}

/// action状態 → DB文字列。
pub fn action_state_str(state: ActionState) -> &'static str {
    match state {
        ActionState::Queued => "queued",
        ActionState::Signing => "signing",
        ActionState::Signed => "signed",
        ActionState::Dispatching => "dispatching",
        ActionState::Reconciled => "reconciled",
        ActionState::Unknown => "unknown",
        ActionState::Aborted => "aborted",
    }
}

/// DB文字列 → action状態。
pub fn action_state_from_str(value: &str) -> Option<ActionState> {
    Some(match value {
        "queued" => ActionState::Queued,
        "signing" => ActionState::Signing,
        "signed" => ActionState::Signed,
        "dispatching" => ActionState::Dispatching,
        "reconciled" => ActionState::Reconciled,
        "unknown" => ActionState::Unknown,
        "aborted" => ActionState::Aborted,
        _ => return None,
    })
}

/// 注文状態 → DB文字列（S3で使う）。
pub fn order_state_str(state: OrderState) -> &'static str {
    match state {
        OrderState::Pending => "pending",
        OrderState::Open => "open",
        OrderState::PartiallyFilled => "partially_filled",
        OrderState::Filled => "filled",
        OrderState::Cancelled => "cancelled",
        OrderState::Rejected => "rejected",
        OrderState::Unknown => "unknown",
    }
}

/// DB文字列 → 注文状態。
pub fn order_state_from_str(value: &str) -> Option<OrderState> {
    Some(match value {
        "pending" => OrderState::Pending,
        "open" => OrderState::Open,
        "partially_filled" => OrderState::PartiallyFilled,
        "filled" => OrderState::Filled,
        "cancelled" => OrderState::Cancelled,
        "rejected" => OrderState::Rejected,
        "unknown" => OrderState::Unknown,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fund_request_states_round_trip() {
        for state in [
            FundRequestState::Accepted,
            FundRequestState::Reserved,
            FundRequestState::Executing,
            FundRequestState::Settled,
            FundRequestState::Rejected,
            FundRequestState::Unknown,
        ] {
            let text = fund_request_state_str(state);
            assert_eq!(fund_request_state_from_str(text), Some(state));
        }
        assert_eq!(fund_request_state_from_str("bogus"), None);
    }

    #[test]
    fn action_states_round_trip() {
        for state in [
            ActionState::Queued,
            ActionState::Signing,
            ActionState::Signed,
            ActionState::Dispatching,
            ActionState::Reconciled,
            ActionState::Unknown,
            ActionState::Aborted,
        ] {
            let text = action_state_str(state);
            assert_eq!(action_state_from_str(text), Some(state));
        }
        assert_eq!(action_state_from_str("bogus"), None);
    }

    #[test]
    fn order_states_round_trip() {
        for state in [
            OrderState::Pending,
            OrderState::Open,
            OrderState::PartiallyFilled,
            OrderState::Filled,
            OrderState::Cancelled,
            OrderState::Rejected,
            OrderState::Unknown,
        ] {
            let text = order_state_str(state);
            assert_eq!(order_state_from_str(text), Some(state));
        }
    }
}
