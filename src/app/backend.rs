mod models;

use crate::utils::sleep_async;
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
    Orders { orders: Vec<super::Order> },
}

// TODO: We should probably keep the url and request parameters here too for better erroring
#[derive(Error, Debug)]
pub enum FetchError {
    #[error("Request failed: {0}")]
    Request(reqwest::Error),

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

            trajectory.extend(page.trades.into_iter().map(models::Trade::into_trade));

            if !page.has_next {
                break;
            }
        }

        self.send_event(BackendEvent::Trades { trajectory });
    }

    pub async fn fetch_orders(&self) {
        let mut orders = Vec::new();
        let mut query = HashMap::new();

        query.insert("srcCurrency", "usdt".to_string());
        query.insert("dstCurrency", "rls".to_string());
        query.insert("details", "2".to_string());

        for page in 1.. {
            query.insert("page", page.to_string());
            let page: OrderPage = self.fetch_endpoint("market/orders/list", &query).await;

            orders.extend(page.orders.into_iter().map(models::Order::into_order));
            if !page.has_next {
                break;
            }
        }

        self.send_event(BackendEvent::Orders { orders })
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
                Err(err) => log::error!("{err}"),
            }

            sleep_async(Duration::from_secs(3)).await;
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

        serde_json::from_str::<T>(&raw_text).map_err(|err| FetchError::Parse(err, raw_text))
    }

    fn send_event(&self, event: BackendEvent) {
        let _ = self.sender.send(event);
        self.ctx.request_repaint()
    }
}
