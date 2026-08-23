#[allow(dead_code)]
#[path = "../app/backend/auth.rs"]
mod auth;

use auth::load_api_credentials;
use eyre::{Context, ContextCompat, Result, bail};
use reqwest::{Client, Method, StatusCode, Url};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::sleep;

const BASE_URL: &str = "https://apiv2.nobitex.ir";
const USER_AGENT: &str = concat!("TraderBot/HolonUI-", env!("CARGO_PKG_VERSION"));
type Exposures = BTreeMap<String, BTreeMap<String, f64>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DatasetStatus {
    name: String,
    endpoint: String,
    state: &'static str,
    records: usize,
    pages: usize,
    error: Option<String>,
}

struct Exporter {
    client: Client,
    base_url: Url,
    credentials: auth::ApiCredentials,
    root: PathBuf,
}

impl Exporter {
    fn new(root: PathBuf) -> Result<Self> {
        let credentials = load_api_credentials();
        credentials
            .validate()
            .wrap_err("no valid Nobitex specialized API key is configured")?;

        Ok(Self {
            client: Client::builder().user_agent(USER_AGENT).build()?,
            base_url: Url::parse(BASE_URL)?,
            credentials,
            root,
        })
    }

    async fn request_json(
        &self,
        method: Method,
        endpoint: &str,
        query: &[(String, String)],
        raw_body: Option<&str>,
    ) -> Result<Value> {
        let url = self.base_url.join(endpoint)?;

        for attempt in 1..=6 {
            let mut request = self
                .client
                .request(method.clone(), url.clone())
                .query(query);
            if let Some(body) = raw_body {
                request = request
                    .header("Content-Type", "application/json")
                    .body(body.to_owned());
            }

            let mut request = request.build()?;
            self.credentials.authenticate(&mut request)?;
            let response = self.client.execute(request).await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get("Retry-After")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            let text = response.text().await?;
            let value: Value = serde_json::from_str(&text)
                .with_context(|| format!("{method} {endpoint} returned non-JSON: {text}"))?;

            let api_failed = value.get("status").and_then(Value::as_str) == Some("failed");
            let backoff = value
                .get("backOff")
                .and_then(Value::as_u64)
                .or_else(|| value.pointer("/error/backOff").and_then(Value::as_u64));
            let rate_limited = status == StatusCode::TOO_MANY_REQUESTS
                || value.get("code").and_then(Value::as_str) == Some("TooManyRequests")
                || backoff.is_some();

            if rate_limited && attempt < 6 {
                let seconds = retry_after.or(backoff).unwrap_or(60).saturating_add(1);
                eprintln!("Rate limited on {endpoint}; retrying in {seconds}s");
                sleep(Duration::from_secs(seconds)).await;
                continue;
            }

            if !status.is_success() || api_failed {
                bail!("{method} {endpoint} failed with HTTP {status}: {value}");
            }

            return Ok(value);
        }

        unreachable!()
    }

    async fn paginated_get(
        &self,
        name: &str,
        endpoint: &str,
        collection_key: &str,
        base_query: &[(&str, &str)],
        page_size: usize,
        max_pages: usize,
    ) -> (DatasetStatus, Vec<Value>) {
        let raw_dir = self.root.join("raw").join(name).join("pages");
        if let Err(err) = fs::create_dir_all(&raw_dir) {
            return (failed_status(name, endpoint, err), Vec::new());
        }

        let mut records = Vec::new();
        let mut completed_pages = 0;

        for page in 1..=max_pages {
            let mut query: Vec<(String, String)> = base_query
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect();
            query.push(("page".to_owned(), page.to_string()));
            query.push(("pageSize".to_owned(), page_size.to_string()));

            let response = match self.request_json(Method::GET, endpoint, &query, None).await {
                Ok(response) => response,
                Err(err) => {
                    return (
                        DatasetStatus {
                            name: name.to_owned(),
                            endpoint: endpoint.to_owned(),
                            state: if records.is_empty() {
                                "failed"
                            } else {
                                "partial"
                            },
                            records: records.len(),
                            pages: completed_pages,
                            error: Some(format!("{err:#}")),
                        },
                        records,
                    );
                }
            };

            let page_path = raw_dir.join(format!("{page:04}.json"));
            if let Err(err) = write_json(&page_path, &response) {
                return (failed_status(name, endpoint, err), records);
            }

            completed_pages += 1;
            let page_records = match response.get(collection_key).and_then(Value::as_array) {
                Some(page_records) => page_records,
                None => {
                    return (
                        DatasetStatus {
                            name: name.to_owned(),
                            endpoint: endpoint.to_owned(),
                            state: "partial",
                            records: records.len(),
                            pages: completed_pages,
                            error: Some(format!("response has no `{collection_key}` array")),
                        },
                        records,
                    );
                }
            };
            records.extend(page_records.iter().cloned());

            if response.get("hasNext").and_then(Value::as_bool) != Some(true) {
                return (
                    DatasetStatus {
                        name: name.to_owned(),
                        endpoint: endpoint.to_owned(),
                        state: "complete",
                        records: records.len(),
                        pages: completed_pages,
                        error: None,
                    },
                    records,
                );
            }
        }

        (
            DatasetStatus {
                name: name.to_owned(),
                endpoint: endpoint.to_owned(),
                state: "partial",
                records: records.len(),
                pages: completed_pages,
                error: Some(format!("reached safety limit of {max_pages} pages")),
            },
            records,
        )
    }

    async fn wallets(&self) -> (DatasetStatus, Vec<Value>) {
        let endpoint = "users/wallets/list";
        let raw_dir = self.root.join("raw/wallets");
        if let Err(err) = fs::create_dir_all(&raw_dir) {
            return (failed_status("wallets", endpoint, err), Vec::new());
        }

        let mut wallets = Vec::new();
        let mut completed = 0;
        let mut errors = Vec::new();

        for wallet_type in ["spot", "margin", "credit", "debit"] {
            let body = format!(r#"{{"type":"{wallet_type}"}}"#);
            match self
                .request_json(Method::POST, endpoint, &[], Some(&body))
                .await
            {
                Ok(response) => {
                    if let Err(err) =
                        write_json(&raw_dir.join(format!("{wallet_type}.json")), &response)
                    {
                        errors.push(format!("{wallet_type}: {err:#}"));
                        continue;
                    }
                    completed += 1;
                    match response.get("wallets").and_then(Value::as_array) {
                        Some(items) => wallets.extend(items.iter().cloned().map(|mut wallet| {
                            if let Some(object) = wallet.as_object_mut() {
                                object.insert(
                                    "walletType".to_owned(),
                                    Value::String(wallet_type.to_owned()),
                                );
                            }
                            wallet
                        })),
                        None => {
                            errors.push(format!("{wallet_type}: response has no `wallets` array"))
                        }
                    }
                }
                Err(err) => errors.push(format!("{wallet_type}: {err:#}")),
            }
        }

        let state = if errors.is_empty() {
            "complete"
        } else if completed == 0 {
            "failed"
        } else {
            "partial"
        };
        (
            DatasetStatus {
                name: "wallets".to_owned(),
                endpoint: endpoint.to_owned(),
                state,
                records: wallets.len(),
                pages: completed,
                error: (!errors.is_empty()).then(|| errors.join("; ")),
            },
            wallets,
        )
    }
}

fn failed_status(name: &str, endpoint: &str, err: impl std::fmt::Display) -> DatasetStatus {
    DatasetStatus {
        name: name.to_owned(),
        endpoint: endpoint.to_owned(),
        state: "failed",
        records: 0,
        pages: 0,
        error: Some(err.to_string()),
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn csv_cell(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn values_to_csv(values: &[Value]) -> String {
    let headers: BTreeSet<String> = values
        .iter()
        .filter_map(Value::as_object)
        .flat_map(|object| object.keys().cloned())
        .collect();
    let headers: Vec<String> = headers.into_iter().collect();
    let mut csv = String::new();
    csv.push_str(
        &headers
            .iter()
            .map(|header| csv_cell(header))
            .collect::<Vec<_>>()
            .join(","),
    );
    csv.push('\n');

    for value in values {
        let Some(object) = value.as_object() else {
            continue;
        };
        csv.push_str(
            &headers
                .iter()
                .map(|header| match object.get(header) {
                    None | Some(Value::Null) => csv_cell(""),
                    Some(Value::String(value)) => csv_cell(value),
                    Some(value) => csv_cell(&value.to_string()),
                })
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push('\n');
    }

    csv
}

fn write_analysis_dataset(root: &Path, name: &str, values: &[Value]) -> Result<()> {
    let analysis = root.join("analysis");
    fs::create_dir_all(&analysis)?;
    write_json(&analysis.join(format!("{name}.json")), &values)?;
    fs::write(analysis.join(format!("{name}.csv")), values_to_csv(values))?;
    Ok(())
}

fn load_exposures(root: &Path) -> Result<(Exposures, Vec<Value>)> {
    let source = Path::new("exposures.yaml");
    let destination = root.join("exposures.yaml");
    fs::copy(source, &destination).wrap_err("failed to copy exposures.yaml")?;
    let raw = fs::read_to_string(source)?;
    let exposures: Exposures = serde_yml::from_str(&raw)?;
    let rows = exposures
        .iter()
        .flat_map(|(currency, sources)| {
            sources.iter().map(move |(source, amount)| {
                json!({
                    "currency": currency,
                    "source": source,
                    "amount": amount,
                })
            })
        })
        .collect();
    Ok((exposures, rows))
}

fn allocation_summary(wallets: &[Value], exposures: &Exposures) -> Vec<Value> {
    let mut currencies: BTreeSet<String> = exposures.keys().cloned().collect();
    currencies.extend(wallets.iter().filter_map(|wallet| {
        wallet
            .get("currency")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }));

    let mut rows = Vec::new();
    for currency in currencies {
        let external_sources = exposures.get(&currency).cloned().unwrap_or_default();
        let external_total: f64 = external_sources.values().sum();
        let currency_wallets: Vec<&Value> = wallets
            .iter()
            .filter(|wallet| wallet.get("currency").and_then(Value::as_str) == Some(&currency))
            .collect();

        if currency_wallets.is_empty() {
            rows.push(json!({
                "currency": currency,
                "walletType": null,
                "exchangeBalance": null,
                "externalTotal": external_total,
                "combinedBalance": external_total,
                "externalSources": external_sources,
            }));
            continue;
        }

        for wallet in currency_wallets {
            let wallet_type = wallet.get("walletType").and_then(Value::as_str);
            let exchange_balance = wallet
                .get("balance")
                .and_then(Value::as_str)
                .and_then(|balance| balance.parse::<f64>().ok());
            let applies_external = wallet_type == Some("spot");
            let combined_balance = exchange_balance.map(|balance| {
                if applies_external {
                    balance + external_total
                } else {
                    balance
                }
            });
            rows.push(json!({
                "currency": currency,
                "walletType": wallet_type,
                "exchangeBalance": wallet.get("balance"),
                "blockedBalance": wallet.get("blockedBalance"),
                "activeBalance": wallet.get("activeBalance"),
                "externalTotal": applies_external.then_some(external_total),
                "combinedBalance": combined_balance,
                "externalSources": applies_external.then_some(&external_sources),
                "rawWallet": wallet,
            }));
        }
    }

    rows
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .wrap_err("usage: cargo run --bin export_nobitex -- <output-directory>")?;
    fs::create_dir_all(root.join("raw"))?;
    fs::create_dir_all(root.join("analysis"))?;

    let exporter = Exporter::new(root.clone())?;
    let created_at = chrono::Utc::now();
    let mut statuses = Vec::new();

    eprintln!("Exporting all available spot trades...");
    let (status, trades) = exporter
        .paginated_get(
            "trades",
            "market/trades/list",
            "trades",
            &[("tradeOrder", "asc")],
            500,
            100,
        )
        .await;
    statuses.push(status);
    write_analysis_dataset(&root, "trades", &trades)?;

    eprintln!("Exporting current open orders...");
    let (status, current_orders) = exporter
        .paginated_get(
            "current_orders",
            "market/orders/list",
            "orders",
            &[
                ("status", "open"),
                ("details", "2"),
                ("order", "created_at"),
            ],
            1000,
            10_000,
        )
        .await;
    statuses.push(status);
    write_analysis_dataset(&root, "current_orders", &current_orders)?;

    eprintln!("Exporting all-status order history for additional context...");
    let (status, all_orders) = exporter
        .paginated_get(
            "all_orders",
            "market/orders/list",
            "orders",
            &[("status", "all"), ("details", "2"), ("order", "created_at")],
            1000,
            10_000,
        )
        .await;
    statuses.push(status);
    write_analysis_dataset(&root, "all_orders", &all_orders)?;

    eprintln!("Exporting wallet allocations...");
    let (status, wallets) = exporter.wallets().await;
    statuses.push(status);
    write_analysis_dataset(&root, "wallets", &wallets)?;

    let (exposures, exposure_rows) = load_exposures(&root)?;
    write_analysis_dataset(&root, "external_exposures", &exposure_rows)?;
    let allocations = allocation_summary(&wallets, &exposures);
    write_analysis_dataset(&root, "allocation_summary", &allocations)?;

    let manifest = json!({
        "createdAt": created_at.to_rfc3339(),
        "baseUrl": BASE_URL,
        "userAgent": USER_AGENT,
        "authentication": "Nobitex specialized API key; credentials are not included",
        "datasets": statuses,
        "files": {
            "raw": "raw/ contains every untouched API page/response",
            "analysis": "analysis/ contains combined JSON arrays and flattened CSV tables",
            "exposures": "exposures.yaml is the verbatim local source file",
        },
        "documentedLimits": {
            "trades": "latest 180 days only; maximum 100 pages; requested 500 records/page",
            "orders": "requested full details and 1000 records/page",
            "wallets": "requested spot, margin, credit, and debit wallet classes",
        },
        "notes": [
            "Raw numeric strings from Nobitex are preserved in raw and analysis JSON/CSV.",
            "combinedBalance in allocation_summary is derived as f64 because exposures.yaml uses floating-point values.",
            "External exposures are added only to the spot wallet row for each currency.",
        ],
    });
    write_json(&root.join("manifest.json"), &manifest)?;

    let incomplete = manifest["datasets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|status| status["state"] != "complete")
        .count();
    eprintln!(
        "Export staged at {} ({} incomplete dataset(s))",
        root.display(),
        incomplete
    );
    Ok(())
}
