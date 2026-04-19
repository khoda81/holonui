mod models;

use crate::{
    app::backend::models::{OrderListResponse, TradesResponse, WalletsResponse},
    utils::sleep_async,
};
use reqwest::Url;
use serde::de::DeserializeOwned;
use std::{str::FromStr, sync::mpsc::Sender, time::Duration};

const BASE_URL: &str = "https://apiv2.nobitex.ir";

pub enum BackendEvent {
    FetchedTrades { trajectory: Vec<super::Trade> },
    FetchedBalances { anchor: super::Allocation },
    FetchedOrders { orders: Vec<models::Order> },
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
            base_url: reqwest::Url::parse("https://apiv2.nobitex.ir/").unwrap(),
        }
    }

    pub async fn fetch_trajectory(&self) {
        let query = [("srcCurrency", "usdt"), ("dstCurrency", "rls")];

        let all_trades = self
            .fetch_paginated("market/trades/list", &query, |res: TradesResponse| {
                (res.trades, res.has_next)
            })
            .await;

        let trajectory = all_trades
            .into_iter()
            .map(models::Trade::into_trade)
            .collect();

        self.send_event(BackendEvent::FetchedTrades { trajectory });
    }

    pub async fn fetch_orders(&self) {
        let query = [
            ("srcCurrency", "usdt"),
            ("dstCurrency", "rls"),
            ("status", "active"),
        ];

        let orders = self
            .fetch_paginated("market/orders/list", &query, |res: OrderListResponse| {
                (res.orders, res.has_next)
            })
            .await;

        self.send_event(BackendEvent::FetchedOrders { orders });
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

    /// Generic helper for fetching all pages of an endpoint
    async fn fetch_paginated<T, R, F>(
        &self,
        endpoint: &str,
        base_query: &[(&str, &str)],
        extract: F,
    ) -> Vec<R>
    where
        T: DeserializeOwned,
        F: Fn(T) -> (Vec<R>, bool), // Extracts (items, has_next) from the response payload
    {
        let mut all_items = Vec::new();

        for page in 1.. {
            let page_str = page.to_string();

            // Build the query for the current page
            let mut current_query = base_query.to_vec();
            current_query.push(("page", &page_str));
            current_query.push(("pageSize", "100"));

            let response: T = self.fetch_endpoint(endpoint, &current_query).await;
            let (items, has_next) = extract(response);

            all_items.extend(items);

            if !has_next {
                log::trace!(
                    "Fetched {} items across {} pages from {}",
                    all_items.len(),
                    page,
                    endpoint
                );
                break;
            }

            sleep_async(Duration::from_millis(200)).await;
        }

        all_items
    }

    /// Base method that hits the endpoint, handles the URL joining, query parameters, and retries.
    async fn fetch_endpoint<T: DeserializeOwned, Q: serde::Serialize + ?Sized>(
        &self,
        endpoint: &str,
        query: &Q,
    ) -> T {
        let url: Url = self
            .base_url
            .join(endpoint)
            .expect("Invalid hardcoded endpoint");

        loop {
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
