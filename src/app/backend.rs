mod models;

use crate::utils::sleep_async;
use chrono::Utc;
use models::{OrderPage, TradesPage, WalletsResponse};
use reqwest::Url;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::{sync::mpsc::Sender, time::Duration};
use thiserror::Error;

const BASE_URL: &str = "https://apiv2.nobitex.ir";

pub enum BackendEvent {
    Trades { trajectory: Vec<super::Trade> },
    Balances { anchor: super::Allocation },
    Orders(super::Orders),
}

#[derive(Error, Debug)]
pub enum FetchError {
    #[error("Request failed: {0}")]
    Request(reqwest::Error),

    #[error("API error ({code}): {message}")]
    APIError { message: String, code: String },

    #[error("Rate limit reached: {message}")]
    RateLimit {
        message: String,
        backoff: chrono::DateTime<Utc>,
        limit: u32,
    },

    #[error("Failed to read response: {0}")]
    Read(reqwest::Error),

    #[error("Failed to parse JSON: {0}\nRaw JSON: {1}")]
    Parse(serde_json::Error, String),
}

pub struct NetworkBackend {
    sender: Sender<BackendEvent>,
    ctx: egui::Context,
    client: reqwest::Client,
    api_key: String,
    base_url: reqwest::Url,
}

impl NetworkBackend {
    pub fn new(sender: Sender<BackendEvent>, ctx: egui::Context) -> Self {
        Self {
            sender,
            ctx,
            client: reqwest::Client::new(),
            api_key: std::env::var("NOBITEX_API_KEY").unwrap_or_default(),
            base_url: reqwest::Url::parse(BASE_URL).unwrap(),
        }
    }

    pub async fn fetch_trajectory(&self) {
        let mut query = HashMap::new();
        query.insert("srcCurrency", "usdt".to_string());
        query.insert("dstCurrency", "rls".to_string());

        let mut trajectory = Vec::new();
        for page in 1.. {
            query.insert("page", page.to_string());
            let page: TradesPage = self.fetch_endpoint("market/trades/list", &query).await;

            trajectory.extend(page.trades.into_iter().map(Into::into));

            if !page.has_next {
                break;
            }
        }

        self.send_event(BackendEvent::Trades { trajectory });
    }

    pub async fn fetch_orders(&self) {
        let mut bids = Vec::new();
        let mut asks = Vec::new();
        let mut query = HashMap::new();

        query.insert("srcCurrency", "usdt".to_string());
        query.insert("dstCurrency", "rls".to_string());
        query.insert("details", "2".to_string());

        for page in 1.. {
            query.insert("page", page.to_string());
            let page: OrderPage = self.fetch_endpoint("market/orders/list", &query).await;

            for order in page.orders {
                match order.order_type {
                    models::TradeType::Buy => bids.push(order),
                    models::TradeType::Sell => asks.push(order),
                }
            }

            if !page.has_next {
                break;
            }
        }

        bids.sort_by(|a, b| b.price.total_cmp(&a.price)); // descending
        asks.sort_by(|a, b| a.price.total_cmp(&b.price)); // ascending

        self.send_event(BackendEvent::Orders(super::Orders {
            bids: bids.into_iter().map(Into::into).collect(),
            asks: asks.into_iter().map(Into::into).collect(),
        }))
    }

    pub async fn fetch_balances(&self) {
        let response: WalletsResponse = self
            .fetch_endpoint("users/wallets/list", &[] as &[(&str, &str)])
            .await;

        let mut current_usdt = 0.0;
        let mut current_rls = 0.0;

        for wallet in response.wallets {
            if wallet.currency == "usdt" {
                current_usdt = wallet.balance;
            } else if wallet.currency == "rls" {
                current_rls = wallet.balance;
            }
        }

        self.send_event(BackendEvent::Balances {
            anchor: [current_usdt, current_rls],
        });
    }

    async fn fetch_endpoint<T, Q>(&self, endpoint: &str, query: &Q) -> T
    where
        T: DeserializeOwned,
        Q: serde::Serialize + ?Sized,
    {
        let url: Url = self
            .base_url
            .join(endpoint)
            .expect("Invalid hardcoded endpoint");

        loop {
            log::trace!("Sending request for: {}", url);

            let request = self
                .client
                .get(url.clone())
                .query(query)
                .header("Authorization", format!("Token {}", self.api_key));

            match self.send_request(request).await {
                Ok(res) => {
                    log::debug!("Fetched successfully: `{}`", url.path());
                    return res;
                }
                Err(err) => log::error!("Error fetching `{}`: {err}", url.path()),
            }

            // TODO: This should be a parameter/constant
            sleep_async(Duration::from_secs(3)).await;
        }
    }

    async fn send_request<T>(&self, request: reqwest::RequestBuilder) -> Result<T, FetchError>
    where
        T: DeserializeOwned,
    {
        // TODO: Add parsing for known failure messages: {"status": "failed", "message": "\u062a\u0639\u062f\u0627\u062f \u062f\u0631\u062e\u0648\u0627\u0633\u062a \u0634\u0645\u0627 \u0628\u06cc\u0634 \u0627\u0632 \u062d\u062f \u0645\u0639\u0645\u0648\u0644 \u062a\u0634\u062e\u06cc\u0635 \u062f\u0627\u062f\u0647 \u0634\u062f\u0647. \u0644\u0637\u0641\u0627 0 \u062b\u0627\u0646\u06cc\u0647 \u0635\u0628\u0631 \u0646\u0645\u0627\u06cc\u06cc\u062f.", "code": "TooManyRequests", "backOff": 0, "limit": 30}
        //       Where the api is telling us that it has failed, we should print the message.

        // TODO: Add a custom error type for rate limits so that the retry loop can wait for exactly that duration instead of retrying blindly

        let raw_text = request
            .send()
            .await
            .map_err(FetchError::Request)?
            .text()
            .await
            .map_err(FetchError::Read)?;

        serde_json::from_str(&raw_text).map_err(|err| FetchError::Parse(err, raw_text))
    }

    fn send_event(&self, event: BackendEvent) {
        let _ = self.sender.send(event);
        self.ctx.request_repaint()
    }
}
