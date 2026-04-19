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

impl Trade {
    pub fn into_trade(self) -> crate::app::Trade {
        let delta = match self.trade_type {
            TradeType::Buy => [-(self.amount - self.fee), self.total],
            TradeType::Sell => [self.amount, -(self.total - self.fee)],
        };

        crate::app::Trade {
            delta,
            timestamp: self.timestamp,
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
pub struct Order {
    #[serde(rename = "type")]
    pub order_type: TradeType,
    #[serde(deserialize_with = "f64_from_str")]
    pub price: f64,
    #[serde(deserialize_with = "f64_from_str")]
    pub amount: f64,
    #[serde(rename = "unmatchedAmount", deserialize_with = "f64_from_str")]
    pub unmatched_amount: f64,
    #[serde(rename = "created_at")]
    pub created_at: DateTime<Utc>,
}

impl Order {
    pub fn into_order(self) -> crate::app::Order {
        let delta = match self.order_type {
            TradeType::Buy => [-(self.amount), todo!()],
            TradeType::Sell => [self.amount, todo!()],
        };

        crate::app::Order {
            delta,
            created_at: self.created_at,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct OrderPage {
    pub orders: Vec<Order>,
    #[serde(rename = "hasNext")]
    pub has_next: bool,
}
