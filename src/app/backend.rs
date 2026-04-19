mod models;

use crate::{
    app::backend::models::{Paginated, WalletsResponse},
    utils::sleep_async,
};
use async_stream::stream;
use futures::Stream;
use futures::StreamExt;
use reqwest::Url;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{sync::mpsc::Sender, time::Duration};

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
        let query = serde_json::json!({
            "srcCurrency": "usdt",
            "dstCurrency": "rls",
        });

        // Pass "trades" as the key
        let mut trade_stream =
            self.fetch_paginated_stream::<models::Trade, _>("market/trades/list", &query, "trades");

        let mut trajectory = Vec::new();
        while let Some(trade) = trade_stream.next() {
            trajectory.push(models::Trade::into_trade(trade));
        }

        self.send_event(BackendEvent::FetchedTrades { trajectory });
    }

    pub async fn fetch_orders(&self) {
        let query = serde_json::json!({
            "srcCurrency": "usdt",
            "dstCurrency": "rls",
            "status": "active",
        });

        // Pass "orders" as the key
        let mut order_stream =
            self.fetch_paginated_stream::<models::Order, _>("market/orders/list", &query, "orders");

        let mut orders = Vec::new();
        while let Some(order) = order_stream.next().await {
            orders.push(order);
        }

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

    pub fn fetch_paginated_stream<'a, T, Q>(
        &'a self,
        endpoint: &'a str,
        query: &'a Q,
        list_key: &'a str, // Pass "orders" or "trades" here
    ) -> impl Stream<Item = T> + 'a
    where
        T: DeserializeOwned + 'a,
        Q: serde::Serialize + ?Sized,
    {
        stream! {
            let mut page_num = 1;

            loop {
                // Convert the base query into a Value so we can append the `page` parameter dynamically
                let mut current_query = serde_json::to_value(query).unwrap_or(serde_json::json!({}));
                current_query["page"] = Value::from(page_num);

                // Fetch the page as an untyped JSON Value
                let response: Value = self.fetch_endpoint(endpoint, &current_query).await;

                // Extract `hasNext`
                let has_next = response
                    .get("hasNext")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                // Dynamically extract the specific list by name and deserialize it into T
                let items: Vec<T> = if let Some(list) = response.get(list_key) {
                    serde_json::from_value(list.clone()).unwrap_or_default()
                } else {
                    Vec::new()
                };

                // Yield items into the stream
                for item in items {
                    yield item;
                }

                if !has_next {
                    break;
                }

                page_num += 1;
            }
        }
    }

    async fn fetch_paginated<T>(
        &self,
        endpoint: &str,
        query: impl IntoIterator<Item = (&str, &str)>,
    ) -> Vec<T>
    where
        T: DeserializeOwned,
    {
        let mut all_items = Vec::new();
        let mut params = Vec::from_iter(query);
        let mut page_str = page.to_string();

        for page in 1.. {
            params.push(("page", &page_str));

            // Fetch the page generically
            let page: Paginated<T> = self.fetch_endpoint(endpoint, &params).await;
            params.pop();

            let has_next = page.has_next;
            // Moves items into all_items efficiently (leaves page.items empty)
            all_items.extend(page.into_items());

            if !has_next {
                break;
            }
        }

        all_items
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
