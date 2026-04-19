use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use thiserror::Error;

// Helper function to deserialize stringified numbers into f64
fn f64_from_str<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

#[derive(Deserialize, Error, Debug)]
#[serde(untagged)]
pub enum ApiFailure {
    #[error("Rate limit reached until {backoff} seconds (limit: {limit})")]
    RateLimit {
        #[serde(rename = "backOff")]
        backoff: u64,
        limit: u32,
    },

    #[error("API error: {0}")]
    Other(Value),
}

#[derive(Deserialize)]
#[serde(tag = "status")]
pub enum ApiResponse<T> {
    #[serde(rename = "ok")]
    Ok(T),

    #[serde(rename = "failed")]
    Failed {
        message: String,
        code: String,

        #[serde(flatten)]
        error: ApiFailure,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TradesPage {
    pub trades: Vec<Trade>,
    #[serde(rename = "hasNext")]
    pub has_next: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Trade {
    pub id: u64,
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

impl From<Trade> for crate::app::Trade {
    fn from(value: Trade) -> Self {
        let delta = match value.trade_type {
            TradeType::Buy => [-(value.amount - value.fee), value.total],
            TradeType::Sell => [value.amount, -(value.total - value.fee)],
        };

        crate::app::Trade {
            delta,
            timestamp: value.timestamp,
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

#[derive(Debug, Deserialize, Clone)]
pub struct OrderPage {
    pub orders: Vec<Order>,
    #[serde(rename = "hasNext")]
    pub has_next: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Order {
    #[serde(rename = "type")]
    pub order_type: TradeType,
    #[serde(deserialize_with = "f64_from_str")]
    pub price: f64,
    #[serde(rename = "unmatchedAmount", deserialize_with = "f64_from_str")]
    pub unmatched_amount: f64,
    #[serde(rename = "created_at")]
    pub created_at: DateTime<Utc>,
}

impl From<Order> for crate::app::Order {
    fn from(value: Order) -> Self {
        // TODO: Apply a fee to the price, these prices are not the actual executed price
        let total = value.unmatched_amount * value.price;

        let delta = match value.order_type {
            TradeType::Buy => [-value.unmatched_amount, total],
            TradeType::Sell => [value.unmatched_amount, -total],
        };

        crate::app::Order {
            delta,
            created_at: value.created_at,
        }
    }
}
