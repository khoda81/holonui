mod models;

use super::Allocation;
use crate::app::backend::models::{TradeType, TradesResponse, WalletsResponse};
use serde::de::DeserializeOwned;
use std::sync::mpsc::Sender;

pub enum BackendEvent {
    FetchedTrades { trajectory: Vec<Allocation> },
    FetchedBalances { anchor: Allocation },
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

    pub(crate) async fn fetch_trajectory(&self) {
        let trades_response: TradesResponse = self
            .fetch_endpoint(
                "https://apiv2.nobitex.ir/market/trades/list?srcCurrency=usdt&dstCurrency=rls",
            )
            .await;

        log::trace!("{trades_response:?}");

        let trajectory = trades_response
            .trades
            .into_iter()
            .map(|trade| {
                // Undo the trade to find the balance BEFORE this trade happened
                match trade.trade_type {
                    // You received $(amount - fee)$ in USDT, and paid $total$ in RLS.
                    // To undo: Subtract the net USDT gained, add the RLS paid.
                    TradeType::Buy => [-(trade.amount - trade.fee), trade.total],
                    // You paid $amount$ in USDT, and received $(total - fee)$ in RLS.
                    // To undo: Add the USDT paid, subtract the net RLS gained.
                    TradeType::Sell => [trade.amount, -(trade.total - trade.fee)],
                }
            })
            .collect();

        self.send_event(BackendEvent::FetchedTrades { trajectory });
    }

    pub(crate) fn send_event(&self, event: BackendEvent) {
        let _ = self.sender.send(event);
        self.ctx.request_repaint()
    }

    pub(crate) async fn fetch_balances(&self) {
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

    pub(crate) async fn fetch_endpoint<T: DeserializeOwned>(&self, url: &str) -> T {
        loop {
            // Group the network request and JSON parsing into a single Result-returning async block
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
                Err(err) => self.send_event(BackendEvent::RequestError(err)),
            }
        }
    }
}
