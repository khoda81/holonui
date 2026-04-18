mod models;

use crate::{
    app::backend::models::{TradesResponse, WalletsResponse},
    utils::sleep_async,
};
use serde::de::DeserializeOwned;
use std::{sync::mpsc::Sender, time::Duration};

pub enum BackendEvent {
    FetchedTrades { trajectory: Vec<super::Trade> },
    FetchedBalances { anchor: super::Allocation },
    RequestError(reqwest::Error),
}

pub struct NetworkBackend {
    sender: Sender<BackendEvent>,
    ctx: egui::Context,
    client: reqwest::Client,
    api_key: String,
}

impl NetworkBackend {
    pub fn new(sender: Sender<BackendEvent>, ctx: egui::Context) -> Self {
        Self {
            sender,
            ctx,
            client: reqwest::Client::new(),
            api_key: std::env::var("NOBITEX_API_KEY").unwrap_or_default(),
        }
    }

    pub async fn fetch_nobitex_data(&self) {
        futures::join!(self.fetch_balances(), self.fetch_trajectory());
    }

    async fn fetch_trajectory(&self) {
        let mut all_trades = Vec::new();

        for page in 1.. {
            // Fetch 100 items per page (the maximum allowed)
            let url = format!(
                "https://apiv2.nobitex.ir/market/trades/list?srcCurrency=usdt&dstCurrency=rls&page={page}&pageSize=100",
            );

            let trades_response: TradesResponse = self.fetch_endpoint(&url).await;
            all_trades.extend(trades_response.trades);

            if !trades_response.has_next {
                log::trace!("Fetched {} trades across {} pages", all_trades.len(), page);
                break;
            }

            // Optional: Be nice to their API and add a tiny delay between pages
            sleep_async(Duration::from_millis(200)).await;
        }

        let trajectory = all_trades
            .into_iter()
            .map(models::Trade::into_trade)
            .collect();

        self.send_event(BackendEvent::FetchedTrades { trajectory });
    }

    fn send_event(&self, event: BackendEvent) {
        let _ = self.sender.send(event);
        self.ctx.request_repaint()
    }

    async fn fetch_balances(&self) {
        // Automatically fetches and parses into WalletsResponse
        let response: WalletsResponse = self
            .fetch_endpoint("https://apiv2.nobitex.ir/users/wallets/list")
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

    async fn fetch_endpoint<T: DeserializeOwned>(&self, url: &str) -> T {
        loop {
            let result = async {
                self.client
                    .get(url)
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
                    // Add the delay before retrying!
                    sleep_async(Duration::from_secs(3)).await;
                }
            }
        }
    }
}
