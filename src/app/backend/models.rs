use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

// Helper function to deserialize stringified numbers into f64
fn f64_from_str<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TradesResponse {
    pub trades: Vec<Trade>,
    #[serde(rename = "hasNext")]
    pub has_next: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Trade {
    #[serde(rename = "type")]
    pub trade_type: TradeType,
    #[serde(deserialize_with = "f64_from_str")]
    pub amount: f64,
    #[serde(deserialize_with = "f64_from_str")]
    pub total: f64,
    #[serde(deserialize_with = "f64_from_str")]
    pub fee: f64,
    pub timestamp: DateTime<Utc>, // Chrono handles the parsing automatically
}

impl Trade {
    pub fn into_trade(trade: Self) -> crate::app::Trade {
        let delta = match trade.trade_type {
            TradeType::Buy => [-(trade.amount - trade.fee), trade.total],
            TradeType::Sell => [trade.amount, -(trade.total - trade.fee)],
        };

        crate::app::Trade {
            delta,
            timestamp: trade.timestamp,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum TradeType {
    Buy,
    Sell,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WalletsResponse {
    pub wallets: Vec<Wallet>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Wallet {
    pub currency: String,
    #[serde(deserialize_with = "f64_from_str")]
    pub balance: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OrderListResponse {
    pub status: String,
    pub orders: Vec<Order>,
    #[serde(rename = "hasNext")]
    pub has_next: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub id: u64,
    #[serde(rename = "type")]
    pub order_type: OrderType,
    #[serde(deserialize_with = "f64_from_str")]
    pub price: f64,
    #[serde(deserialize_with = "f64_from_str")]
    pub amount: f64,
    #[serde(deserialize_with = "f64_from_str")]
    pub unmatched_amount: f64,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OrderType {
    Buy,
    Sell,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum OrderStatus {
    Active,
    Open,
    Done,
    Close,
    Canceled,
    // TODO: Can we store the unmatched value for better error handling?
    #[serde(other)]
    Unknown,
}
