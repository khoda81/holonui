pub mod models;

use super::{
    backend::models::{ApiFailure, ApiResponse},
    plot::OrderFrontier,
};
use crate::{
    app::plot::{Allocation, Trajectory},
    utils::{get_api_key, sleep_async, spawn_task},
};
use futures::channel::mpsc::UnboundedSender;
use models::{OrderPage, TradesPage, WalletsResponse};
use reqwest::Url;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

const BASE_URL: &str = "https://apiv2.nobitex.ir";
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(3);

#[derive(Error, Debug)]
pub enum FetchError {
    #[error("Unauthorized")]
    Unauthorized,

    #[error("HTTP error {status}: {body}")]
    Http {
        status: reqwest::StatusCode,
        body: String,
    },

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

pub enum BackendCommand {
    FetchAll,
    UpdateApiKey(String),
}

pub enum BackendEvent {
    Trades(Trajectory),
    Balances(Allocation),
    Orders(OrderFrontier),
    NeedApiKey,
}

pub struct NetworkBackend {
    events: std::sync::mpsc::Sender<BackendEvent>,
    commands: tokio::sync::mpsc::Receiver<BackendCommand>,
    ctx: egui::Context,
    client: reqwest::Client,
    base_url: reqwest::Url,
}

impl NetworkBackend {
    pub fn new(
        events: std::sync::mpsc::Sender<BackendEvent>,
        commands: tokio::sync::mpsc::Receiver<BackendCommand>,
        ctx: egui::Context,
    ) -> Self {
        Self {
            events,
            commands,
            ctx,
            client: reqwest::Client::new(),
            base_url: reqwest::Url::parse(BASE_URL).unwrap(),
        }
    }

    pub async fn run_backend(&mut self) {
        let mut cancel_token = CancellationToken::new();
        let mut api_token = get_api_key().unwrap_or_default();

        // The main event loop
        while let Some(cmd) = self.commands.recv().await {
            match cmd {
                BackendCommand::FetchAll => {
                    let tx_clone = self.events.clone();
                    let client_clone = self.client.clone();
                    let ctx_clone = self.ctx.clone();

                    spawn_task(async move {
                        // Do async network request...
                        // let data = fetch_something(&client_clone).await;

                        // Send sync event back to UI
                        // tx_clone.send(BackendEvent::Trades(data)).unwrap();
                    });
                }
                BackendCommand::UpdateApiKey(new_token) => {
                    cancel_token.cancel(); // Kills all pending spawns
                    cancel_token = CancellationToken::new(); // Reset
                    api_token = new_token;
                }
            }
        }
    }

    async fn fetch_trajectory(&self) {
        let mut query = HashMap::new();
        query.insert("srcCurrency", "usdt".to_string());
        query.insert("dstCurrency", "rls".to_string());

        let mut trajectory = Vec::new();
        for page in 1.. {
            query.insert("page", page.to_string());
            let page: TradesPage = self.fetch_endpoint("market/trades/list", &query).await;

            trajectory.extend(page.trades.into_iter().map(Into::into));

            self.send_event(BackendEvent::Trades(trajectory.clone()));

            if !page.has_next {
                break;
            }
        }
    }

    async fn fetch_orders(&self) {
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

        self.send_event(BackendEvent::Orders(OrderFrontier::from_sides(bids, asks)))
    }

    async fn fetch_balances(&self) {
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

        self.send_event(BackendEvent::Balances([current_usdt, current_rls]));
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
            log::trace!("Sending request for: {url} (token: {})", self.api_key);

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
        // TODO: We should probably have some mutex/semaphore to limit concurrent connections
        let resp = request.send().await.map_err(FetchError::Request)?;

        let status = resp.status();
        let raw_text = resp.text().await.map_err(FetchError::Read)?;

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            self.send_event(BackendEvent::NeedApiKey);
            return Err(FetchError::Unauthorized);
        }

        if !status.is_success() {
            return Err(FetchError::Http {
                status,
                body: raw_text,
            });
        }

        let response: ApiResponse<T> =
            serde_json::from_str(&raw_text).map_err(|e| FetchError::Parse(e, raw_text))?;

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
        if self.events.send(event).is_ok() {
            self.ctx.request_repaint();
        }
    }
}
