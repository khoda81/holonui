use serde::{Deserialize, Deserializer};

// Helper function to deserialize stringified numbers into f64
fn f64_from_str<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

#[derive(Deserialize, Debug)]
pub struct TradesResponse {
    pub trades: Vec<Trade>,
}

#[derive(Deserialize, Debug)]
pub struct Trade {
    #[serde(rename = "type")]
    pub trade_type: TradeType,
    #[serde(deserialize_with = "f64_from_str")]
    pub amount: f64,
    #[serde(deserialize_with = "f64_from_str")]
    pub total: f64,
    #[serde(deserialize_with = "f64_from_str")]
    pub fee: f64,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TradeType {
    Buy,
    Sell,
}

#[derive(Deserialize, Debug)]
pub struct WalletsResponse {
    pub wallets: Vec<Wallet>,
}

#[derive(Deserialize, Debug)]
pub struct Wallet {
    pub currency: String,
    #[serde(deserialize_with = "f64_from_str")]
    pub balance: f64,
}
