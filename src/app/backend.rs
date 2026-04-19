mod models;

use crate::app::backend::models::{OrderPage, TradesPage};
use crate::{app::backend::models::WalletsResponse, utils::sleep_async};
use reqwest::Url;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::{sync::mpsc::Sender, time::Duration};

const BASE_URL: &str = "https://apiv2.nobitex.ir";

pub enum BackendEvent {
    FetchedTrades { trajectory: Vec<super::Trade> },
    FetchedBalances { anchor: super::Allocation },
    FetchedOrders { orders: Vec<super::Order> },
    RequestError(reqwest::Error),
}

pub struct NetworkBackend {
    sender: Sender<BackendEvent>,
    ctx: egui::Context,
    client: reqwest::Client,
    api_key: String,
    base_url: reqwest::Url, // Store parsed URL here
}

impl NetworkBackend {
    pub fn new(sender: Sender<BackendEvent>, ctx: egui::Context) -> Self {
        Self {
            sender,
            ctx,
            client: reqwest::Client::new(),
            api_key: std::env::var("NOBITEX_API_KEY").unwrap_or_default(),
            // Parse once during initialization
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

        self.send_event(BackendEvent::FetchedTrades { trajectory });
    }

    pub async fn fetch_orders(&self) {
        let mut orders = Vec::new();
        let query: &[(&str, &str)] = &[];

        loop {
            let page: OrderPage = self.fetch_endpoint("market/orders/list", query).await;

            orders.extend(page.orders.into_iter().map(models::Order::into_order));
            if !page.has_next {
                break;
            }
        }

        self.send_event(BackendEvent::FetchedOrders { orders })
    }

    pub async fn fetch_balances(&self) {
        // No query parameters needed here, so we pass an empty array
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

        self.send_event(BackendEvent::FetchedBalances {
            anchor: [current_usdt, current_rls],
        });
    }

    /// Base method that hits the endpoint, handles the URL joining, query parameters, and retries.
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
            log::trace!("Sending request for: {}", url.clone());

            let result = async {
                self.client
                    .get(url.clone())
                    .query(query)
                    .header("Authorization", format!("Token {}", self.api_key))
                    .send()
                    .await?
                    .json::<T>()
                    .await
            };

            match result.await {
                Ok(parsed) => break parsed,
                Err(err) => {
                    self.send_event(BackendEvent::RequestError(err));
                    sleep_async(Duration::from_secs(3)).await;
                }
            }
        }
    }

    fn send_event(&self, event: BackendEvent) {
        let _ = self.sender.send(event);
        self.ctx.request_repaint()
    }
}
