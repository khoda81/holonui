mod models;

use crate::app::backend::models::{ApiFailure, ApiResponse};
use crate::utils::sleep_async;
use models::{OrderPage, TradesPage, WalletsResponse};
use reqwest::Url;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::{sync::mpsc::Sender, time::Duration};
use thiserror::Error;

const BASE_URL: &str = "https://apiv2.nobitex.ir";
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(3);

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
    APIError {
        message: String,
        code: String,
        #[source]
        error: ApiFailure,
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

            self.send_event(BackendEvent::Trades {
                trajectory: trajectory.clone(),
            });

            if !page.has_next {
                break;
            }
        }
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

            let err = match self.send_request(request).await {
                Ok(res) => {
                    log::debug!("Fetched successfully: `{}`", url.path());
                    return res;
                }

                Err(err) => err,
            };

            let mut retry_delay = DEFAULT_RETRY_DELAY;

            match err {
                FetchError::APIError {
                    error: ApiFailure::RateLimit { backoff, .. },
                    ..
                } => {
                    retry_delay = Duration::from_secs(backoff + 1);

                    log::warn!(
                        "API limit reached for {}: waiting {} seconds for retry",
                        url.path(),
                        retry_delay.as_secs()
                    )
                }

                FetchError::APIError { message, code, .. } => {
                    log::error!("API error fetching `{}` ({code}): {message}", url.path())
                }

                err => log::error!("Error fetching `{}`: {err}", url.path()),
            }

            sleep_async(retry_delay).await;
        }
    }

    async fn send_request<T>(&self, request: reqwest::RequestBuilder) -> Result<T, FetchError>
    where
        T: DeserializeOwned,
    {
        let raw_text = request
            .send()
            .await
            .map_err(FetchError::Request)?
            .text()
            .await
            .map_err(FetchError::Read)?;

        let response: ApiResponse<T> =
            serde_json::from_str(&raw_text).map_err(|err| FetchError::Parse(err, raw_text))?;

        match response {
            ApiResponse::Ok(data) => Ok(data),

            ApiResponse::Failed {
                message,
                error,
                code,
            } => Err(FetchError::APIError {
                message,
                error,
                code,
            }),
        }
    }

    fn send_event(&self, event: BackendEvent) {
        let _ = self.sender.send(event);
        self.ctx.request_repaint()
    }
}
