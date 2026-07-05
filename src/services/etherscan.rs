use crate::models::TxRecord;
use anyhow::{anyhow, Context, Result};
use chrono::{TimeZone, Utc};

const BASE_URL: &str = "https://api.etherscan.io/api";

/// Native ETH balance, denominated in ETH (not wei).
pub async fn get_eth_balance(client: &reqwest::Client, api_key: &str, address: &str) -> Result<f64> {
    let url = format!(
        "{}?module=account&action=balance&address={}&tag=latest&apikey={}",
        BASE_URL, address, api_key
    );
    let resp: serde_json::Value = client.get(&url).send().await?.json().await?;
    let wei_str = resp
        .get("result")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("unexpected etherscan balance response: {resp}"))?;
    let wei: u128 = wei_str.parse().context("failed to parse wei balance")?;
    Ok(wei as f64 / 1e18)
}

/// Normal (native ETH) transactions for an address, most recent first.
pub async fn get_eth_transactions(
    client: &reqwest::Client,
    api_key: &str,
    address: &str,
    limit: usize,
) -> Result<Vec<TxRecord>> {
    let url = format!(
        "{}?module=account&action=txlist&address={}&startblock=0&endblock=99999999&page=1&offset={}&sort=desc&apikey={}",
        BASE_URL, address, limit, api_key
    );
    let resp: serde_json::Value = client.get(&url).send().await?.json().await?;
    let items = resp
        .get("result")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let addr_lower = address.to_lowercase();
    let mut out = Vec::new();
    for item in items {
        let from = item.get("from").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        let to = item.get("to").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        let value_wei: u128 = item
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        let timestamp: i64 = item
            .get("timeStamp")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);

        let direction = if to == addr_lower { "in" } else { "out" };
        let counterparty = if direction == "in" { from.clone() } else { to.clone() };

        out.push(TxRecord {
            hash: item.get("hash").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            timestamp: Utc.timestamp_opt(timestamp, 0).single().unwrap_or_else(Utc::now),
            direction: direction.to_string(),
            asset: "ETH".to_string(),
            amount: value_wei as f64 / 1e18,
            counterparty,
        });
    }
    Ok(out)
}

/// ERC-20 transfer events, used both for tx history and as an approximation
/// of current token holdings (summing in/out transfers per token symbol).
pub async fn get_erc20_transfers(
    client: &reqwest::Client,
    api_key: &str,
    address: &str,
    limit: usize,
) -> Result<Vec<serde_json::Value>> {
    let url = format!(
        "{}?module=account&action=tokentx&address={}&startblock=0&endblock=99999999&page=1&offset={}&sort=desc&apikey={}",
        BASE_URL, address, limit, api_key
    );
    let resp: serde_json::Value = client.get(&url).send().await?.json().await?;
    Ok(resp.get("result").and_then(|v| v.as_array()).cloned().unwrap_or_default())
}
