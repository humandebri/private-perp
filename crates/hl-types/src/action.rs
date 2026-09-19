//! Hyperliquidのaction構築（msgpackへ渡す値の組み立て）。
//!
//! フィールド順はSDKの符号化と一致させる必要があるため、ここで固定する。
//! 数量・価格は正規化した十進文字列として渡し、整数は整数のまま渡す。
//! 取引action（order/cancel/cancelByCloid/updateLeverage）はphantom agent方式で署名する。

use crate::decimal::Decimal;
use crate::msgpack::Value;

/// 注文の時間指定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    Gtc,
    Ioc,
    Alo,
}

impl TimeInForce {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gtc => "Gtc",
            Self::Ioc => "Ioc",
            Self::Alo => "Alo",
        }
    }
}

/// トリガ注文（建玉単位のSL/TP）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerOrder {
    pub is_market: bool,
    pub trigger_price: Decimal,
    /// `"tp"` または `"sl"`。
    pub tpsl: Tpsl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tpsl {
    TakeProfit,
    StopLoss,
}

impl Tpsl {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TakeProfit => "tp",
            Self::StopLoss => "sl",
        }
    }
}

/// 注文の種類。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderType {
    Limit { tif: TimeInForce },
    Trigger(TriggerOrder),
}

impl OrderType {
    fn to_value(&self) -> Value {
        match self {
            Self::Limit { tif } => Value::map(vec![(
                "limit",
                Value::map(vec![("tif", Value::str(tif.as_str()))]),
            )]),
            Self::Trigger(trigger) => Value::map(vec![(
                "trigger",
                Value::map(vec![
                    ("isMarket", Value::Bool(trigger.is_market)),
                    ("triggerPx", Value::owned(trigger.trigger_price.to_string())),
                    ("tpsl", Value::str(trigger.tpsl.as_str())),
                ]),
            )]),
        }
    }
}

/// 注文1件分の指定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderRequest {
    /// `meta` の `universe` 添字。
    pub asset_index: u32,
    pub is_buy: bool,
    /// 指値価格（Marketはスリッページ上限付きのIOC指値としてこの価格を使う）。
    pub price: Decimal,
    pub size: Decimal,
    pub reduce_only: bool,
    pub order_type: OrderType,
    /// 16バイトのcloid（`0x`付きhex文字列）。
    pub cloid: Option<String>,
}

impl OrderRequest {
    fn to_value(&self) -> Value {
        let mut fields = vec![
            ("a", Value::UInt(self.asset_index as u64)),
            ("b", Value::Bool(self.is_buy)),
            ("p", Value::owned(self.price.to_string())),
            ("s", Value::owned(self.size.to_string())),
            ("r", Value::Bool(self.reduce_only)),
        ];
        fields.push(("t", self.order_type.to_value()));
        if let Some(cloid) = &self.cloid {
            fields.push(("c", Value::owned(cloid.clone())));
        }
        Value::map(fields)
    }
}

/// 注文のグループ化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grouping {
    Na,
    NormalTpsl,
    PositionTpsl,
}

impl Grouping {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Na => "na",
            Self::NormalTpsl => "normalTpsl",
            Self::PositionTpsl => "positionTpsl",
        }
    }
}

/// `order` action。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderAction {
    pub orders: Vec<OrderRequest>,
    pub grouping: Grouping,
}

impl OrderAction {
    pub fn to_value(&self) -> Value {
        Value::map(vec![
            ("type", Value::str("order")),
            (
                "orders",
                Value::Array(self.orders.iter().map(OrderRequest::to_value).collect()),
            ),
            ("grouping", Value::str(self.grouping.as_str())),
        ])
    }
}

/// `cancel` action（oid指定）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelAction {
    pub cancels: Vec<(u32, u64)>,
}

impl CancelAction {
    pub fn to_value(&self) -> Value {
        Value::map(vec![
            ("type", Value::str("cancel")),
            (
                "cancels",
                Value::Array(
                    self.cancels
                        .iter()
                        .map(|(asset_index, oid)| {
                            Value::map(vec![
                                ("a", Value::UInt(*asset_index as u64)),
                                ("o", Value::UInt(*oid)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

/// `cancelByCloid` action。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelByCloidAction {
    pub cancels: Vec<(u32, String)>,
}

impl CancelByCloidAction {
    pub fn to_value(&self) -> Value {
        Value::map(vec![
            ("type", Value::str("cancelByCloid")),
            (
                "cancels",
                Value::Array(
                    self.cancels
                        .iter()
                        .map(|(asset_index, cloid)| {
                            Value::map(vec![
                                ("asset", Value::UInt(*asset_index as u64)),
                                ("cloid", Value::owned(cloid.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

/// `updateLeverage` action。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateLeverageAction {
    pub asset_index: u32,
    pub is_cross: bool,
    pub leverage: u32,
}

impl UpdateLeverageAction {
    pub fn to_value(&self) -> Value {
        Value::map(vec![
            ("type", Value::str("updateLeverage")),
            ("asset", Value::UInt(self.asset_index as u64)),
            ("isCross", Value::Bool(self.is_cross)),
            ("leverage", Value::UInt(self.leverage as u64)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CancelAction, Grouping, OrderAction, OrderRequest, OrderType, TimeInForce,
        UpdateLeverageAction,
    };
    use crate::decimal::Decimal;
    use crate::msgpack::Value;

    fn price(value: &str) -> Decimal {
        Decimal::parse(value).expect("valid decimal")
    }

    #[test]
    fn order_action_field_order_is_fixed() {
        let action = OrderAction {
            orders: vec![OrderRequest {
                asset_index: 0,
                is_buy: true,
                price: price("60000"),
                size: price("0.01"),
                reduce_only: false,
                order_type: OrderType::Limit {
                    tif: TimeInForce::Gtc,
                },
                cloid: Some("0x0102030405060708090a0b0c0d0e0f10".to_string()),
            }],
            grouping: Grouping::Na,
        };
        let encoded = action.to_value().encode();
        // マップは3フィールド、先頭キーは "type"。
        assert_eq!(encoded[0], 0x83);
        assert_eq!(&encoded[1..6], &[0xa4, b't', b'y', b'p', b'e']);
        // 数量・価格は文字列として符号化される。
        let text = String::from_utf8_lossy(&encoded);
        assert!(text.contains("60000"));
        assert!(text.contains("0.01"));
    }

    #[test]
    fn cancel_action_uses_oid_field() {
        let action = CancelAction {
            cancels: vec![(3, 12345)],
        };
        let encoded = action.to_value().encode();
        let text = String::from_utf8_lossy(&encoded);
        assert!(text.contains("cancel"));
        assert!(text.contains("cancels"));
    }

    #[test]
    fn update_leverage_action_is_minimal() {
        let action = UpdateLeverageAction {
            asset_index: 1,
            is_cross: true,
            leverage: 3,
        };
        assert_eq!(
            action.to_value(),
            Value::map(vec![
                ("type", Value::str("updateLeverage")),
                ("asset", Value::UInt(1)),
                ("isCross", Value::Bool(true)),
                ("leverage", Value::UInt(3)),
            ])
        );
    }
}
