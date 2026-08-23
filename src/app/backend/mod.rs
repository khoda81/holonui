mod auth;
pub mod models;

use super::plot::{Allocation, OrderFrontier, Trajectory};
use crate::utils::{sleep_async, spawn_task};
use auth::load_api_credentials;
use models::{ApiFailure, ApiResponse, OrderPage, TradesPage, WalletsResponse};
use reqwest::Url;
use serde::de::DeserializeOwned;
use std::time::Duration;
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;
use tokio::sync::{RwLock, mpsc as tmpsc};
use tokio_util::sync::CancellationToken;

pub(super) use auth::ApiCredentials;
#[cfg(not(target_arch = "wasm32"))]
pub(super) use auth::save_api_credentials;

const BASE_URL: &str = "https://apiv2.nobitex.ir";
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(3);

/// The trading pair shown by the app and queried from the API.
/// `src` is the source currency (plot X axis), `dst` is the destination
/// currency (plot Y axis).
#[derive(Clone, Debug)]
pub struct Market {
    pub src: String,
    pub dst: String,
}

impl Default for Market {
    fn default() -> Self {
        Self {
            src: "usdt".into(),
            dst: "rls".into(),
        }
    }
}

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
    CancelAll,
    #[cfg(not(target_arch = "wasm32"))]
    UpdateApiCredentials(ApiCredentials),
}

pub enum BackendEvent {
    Trades(Trajectory),
    Balances(Allocation),
    Orders(OrderFrontier),
    NeedApiKey,
}

/// State that is shared across all tasks and the main backend loop.
/// Arc lets us share ownership, and RwLock provides safe interior mutability.
struct BackendState {
    api_credentials: RwLock<ApiCredentials>,
    client: reqwest::Client,
    base_url: reqwest::Url,
    market: Market,
}

/// A handle that is cloned and given to each spawned task.
/// Cloning this is very cheap (just atomic reference count bumps).
#[derive(Clone)]
struct RequestContext {
    state: Arc<BackendState>,
    events: tmpsc::Sender<BackendEvent>,
    ctx: egui::Context,
}

async fn cancelable<F: Future>(f: F, cancel_token: CancellationToken) {
    tokio::select! {
        _ = cancel_token.cancelled() => {  }
        _ = f => {  }
    }
}

pub async fn start_backend(
    ctx: egui::Context,
    market: Market,
    mut commands: tmpsc::UnboundedReceiver<BackendCommand>,
    events: tmpsc::Sender<BackendEvent>,
) {
    let state = Arc::new(BackendState {
        api_credentials: RwLock::new(load_api_credentials()),
        client: reqwest::Client::new(),
        base_url: reqwest::Url::parse(BASE_URL).unwrap(),
        market,
    });

    let mut cancel_token = CancellationToken::new();

    let context = RequestContext { events, state, ctx };

    while let Some(cmd) = commands.recv().await {
        match cmd {
            BackendCommand::FetchAll => {
                spawn_task(cancelable(
                    context.clone().fetch_balances(),
                    cancel_token.clone(),
                ));
                spawn_task(cancelable(
                    context.clone().fetch_orders(),
                    cancel_token.clone(),
                ));
                spawn_task(cancelable(
                    context.clone().fetch_trajectory(),
                    cancel_token.clone(),
                ));
            }

            #[cfg(not(target_arch = "wasm32"))]
            BackendCommand::UpdateApiCredentials(new_credentials) => {
                cancel_token.cancel(); // Kills all pending spawns
                cancel_token = CancellationToken::new(); // Reset

                let mut credentials = context.state.api_credentials.write().await;
                *credentials = new_credentials;
            }

            BackendCommand::CancelAll => {
                cancel_token.cancel();
                cancel_token = CancellationToken::new();
            }
        }
    }
}

impl RequestContext {
    async fn fetch_trajectory(self) {
        let market = &self.state.market;
        let mut query = HashMap::new();
        query.insert("srcCurrency", market.src.clone());
        query.insert("dstCurrency", market.dst.clone());

        let mut trajectory = Vec::new();
        // TODO: Limit pagination based on time
        for page in 1..5 {
            query.insert("page", page.to_string());
            let page: TradesPage = self.fetch_endpoint("market/trades/list", &query).await;

            trajectory.extend(page.trades.into_iter().map(Into::into));
            self.send_event(BackendEvent::Trades(trajectory.clone()))
                .await;

            if !page.has_next {
                break;
            }
        }
    }

    async fn fetch_orders(self) {
        let market = &self.state.market;
        let mut bids = Vec::new();
        let mut asks = Vec::new();
        let mut query = HashMap::new();

        query.insert("srcCurrency", market.src.clone());
        query.insert("dstCurrency", market.dst.clone());
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

        log::trace!("bids: {:#?}", bids);
        log::trace!("asks: {:#?}", asks);

        self.send_event(BackendEvent::Orders(OrderFrontier::from_sides(bids, asks)))
            .await
    }

    async fn fetch_balances(self) {
        let market = &self.state.market;
        let response: WalletsResponse = self
            .fetch_endpoint("users/wallets/list", &[] as &[(&str, &str)])
            .await;

        let mut current_src = 0.0;
        let mut current_dst = 0.0;

        for wallet in response.wallets {
            if wallet.currency == market.src {
                current_src += wallet.balance;
            } else if wallet.currency == market.dst {
                current_dst += wallet.balance;
            }
        }

        self.send_event(BackendEvent::Balances([current_src, current_dst]))
            .await;
    }

    async fn fetch_endpoint<T, Q>(&self, endpoint: &str, query: &Q) -> T
    where
        T: DeserializeOwned,
        Q: serde::Serialize + ?Sized,
    {
        let url: Url = self
            .state
            .base_url
            .join(endpoint)
            .expect("Invalid hardcoded endpoint");

        loop {
            log::trace!("Sending request for: {url}");

            let mut request = match self.state.client.get(url.clone()).query(query).build() {
                Ok(request) => request,
                Err(err) => {
                    log::error!("Failed to build request for `{}`: {err}", url.path());
                    sleep_async(DEFAULT_RETRY_DELAY).await;
                    continue;
                }
            };

            let credentials = self.state.api_credentials.read().await.clone();
            if let Err(err) = credentials.authenticate(&mut request) {
                log::error!("Failed to authenticate request for `{}`: {err}", url.path());
                self.send_event(BackendEvent::NeedApiKey).await;
                sleep_async(DEFAULT_RETRY_DELAY).await;
                continue;
            }

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

    async fn send_request<T>(&self, request: reqwest::Request) -> Result<T, FetchError>
    where
        T: DeserializeOwned,
    {
        // TODO: We should probably have some mutex/semaphore to limit concurrent connections
        let resp = self
            .state
            .client
            .execute(request)
            .await
            .map_err(FetchError::Request)?;

        let status = resp.status();
        let raw_text = resp.text().await.map_err(FetchError::Read)?;

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            self.send_event(BackendEvent::NeedApiKey).await;
            return Err(FetchError::Unauthorized);
        }

        if !status.is_success() {
            if let Ok(ApiResponse::<T>::Failed {
                message,
                code,
                error,
            }) = serde_json::from_str(&raw_text)
            {
                return Err(FetchError::APIError {
                    message,
                    error,
                    code,
                });
            }

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

    async fn send_event(&self, event: BackendEvent) {
        self.events
            .send(event)
            .await
            .is_ok()
            .then(|| self.ctx.request_repaint());
    }
}
