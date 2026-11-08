<div align="center">

# HolonUI

**See your Nobitex portfolio as geometry.**

A native Rust + [egui](https://github.com/emilk/egui) app that draws your balances, open orders and trade history
in balance space, and shows how your allocation would change as the price moves.

![Rust](https://img.shields.io/badge/rust-2024_edition-b7410e?logo=rust)
![egui](https://img.shields.io/badge/ui-egui_0.34-4b8bbe)
![Exchange](https://img.shields.io/badge/exchange-Nobitex-1f6feb)
![Status](https://img.shields.io/badge/status-experimental-orange)

</div>

---

## Why

An order book shows prices and sizes. What you usually care about is your own position: *how much of my value is in
USDT, and what happens to it if my orders fill?* HolonUI answers that by plotting the portfolio itself, not the market.

## The two charts

### Skyline: balance space

The main plot puts one currency on each axis (by default **USDT** on X and **rials** on Y), so every point is a
possible portfolio.

| Element | Meaning |
| --- | --- |
| **Anchor** (gold) | Your current balance: Nobitex wallets plus any off-exchange holdings from `exposures.yaml`. |
| **Order chains** (green bids, red asks) | Your open orders, each drawn as the segment the balance would travel along if it fills, chained outward from the anchor. |
| **Trajectory** (blue) | Your past trades walked backwards from the anchor, fading with age. |

It uses a log–log grid with a 1:1 aspect by default (toggle it in the sidebar); double-click to re-frame.

### Price–allocation curve

The narrow chart on the right shows, for every price (log scale, vertical), the fraction of your total value
held in the source currency (horizontal), assuming the price moved there and every order it passed through filled.

With balance $x$ of the source currency, $y$ of the destination currency and price $p$, the allocation is

$$f(p) = \frac{x\,p}{x\,p + y}$$

Each open order adds a horizontal step where it fills. The trading fee is included, so prices are the effective prices you
actually get.

## Getting started

You need a recent stable Rust toolchain (edition 2024, Rust 1.85 or newer).

```sh
cargo run --release
```

### API key

HolonUI authenticates with a Nobitex **specialized API key** (Ed25519). Charting only needs the `READ` permission.

- Run the app and paste the key into the dialog that appears. It is stored in your system keyring.
- Or set `NOBITEX_PUBLIC_KEY` and `NOBITEX_PRIVATE_KEY` in the environment or in a `.env` file.

### Off-exchange holdings (optional)

Balances the API can't see, such as bank accounts or other wallets, can be added in `exposures.yaml` in the working directory.
Amounts are grouped by currency and summed into the anchor. The file is git-ignored.

```yaml
rls:
  bank_account: 120000000
usdt:
  hardware_wallet: 250.0
```

## Controls

| Input | Action |
| --- | --- |
| <kbd>F5</kbd> | Refetch balances, orders and trades |
| Double-click the skyline | Re-frame around the anchor and orders |
| Drag / scroll the price chart | Pan / zoom the price axis |
| Sidebar | Trajectory fade time, grid scale, show/hide layers |

## Data export

A separate binary dumps your account data for offline analysis:

```sh
cargo run --release --bin export_nobitex -- exports/my-export
```

It writes untouched API pages under `raw/`, combined JSON and flattened CSV tables under `analysis/` (trades, open and
historical orders, wallets, and an allocation summary that includes `exposures.yaml`), and a `manifest.json` describing
what was fetched. Credentials are never written.

## Project layout

```text
src/
├── app.rs                    # App state, panels, input
├── app/backend/              # Async Nobitex client (tokio / wasm), request signing, API models
├── app/plot/                 # Skyline and price–allocation rendering
├── utils.rs                  # Log grid spacing, number formatting, exposures loading
└── bin/export_nobitex.rs     # Account data exporter
```

## Roadmap

- [Click-to-trade on the price–allocation chart](https://github.com/khoda81/holonui/issues/1): every point is an
  allocation at a price, so it corresponds to exactly one limit order.
- [Live updates from Nobitex private WebSockets](https://github.com/khoda81/holonui/issues/2) so manual refreshes
  aren't needed.
- A working web (WASM / Trunk) build. The code is structured for it, but it does not build or authenticate yet.

> [!WARNING]
> HolonUI is a personal, experimental tool and is not affiliated with Nobitex. Check anything it shows against the
> exchange before acting on it.
