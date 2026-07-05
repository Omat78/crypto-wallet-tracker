use anyhow::{Context, Result};
use std::collections::HashMap;

/// Fetch USD prices for a set of CoinGecko coin ids, e.g. ["ethereum", "solana", "usd-coin"].
pub async fn get_usd_prices(
    client: &reqwest::Client,
    api_base: &str,
    ids: &[&str],
) -> Result<HashMap<String, f64>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let ids_param = ids.join(",");
    let url = format!(
        "{}/simple/price?ids={}&vs_currencies=usd",
        api_base, ids_param
    );

    let resp: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .context("coingecko request failed")?
        .json()
        .await
        .context("coingecko response was not valid json")?;

    let mut out = HashMap::new();
    if let Some(obj) = resp.as_object() {
        for (coin_id, val) in obj {
            if let Some(price) = val.get("usd").and_then(|v| v.as_f64()) {
                out.insert(coin_id.clone(), price);
            }
        }
    }
    Ok(out)
}
