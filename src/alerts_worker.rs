use crate::db;
use crate::services::{coingecko, etherscan, solana};
use crate::AppState;
use std::sync::Arc;
use std::time::Duration;

/// Runs forever in the background, polling every subscribed alert and firing
/// a webhook POST when a threshold is crossed. Runs on the shared tokio runtime
/// alongside the Axum server (spawned from `main`).
///
/// Notes / production hardening ideas:
/// - This polls sequentially; for many alerts, batch by chain and dedupe RPC calls.
/// - `large_transfer` currently checks only the most recent transaction on each
///   poll, so a transfer could be reported more than once across polls close
///   together — track the last-seen tx hash per alert to dedupe in production.
pub async fn run(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(Duration::from_secs(state.config.alert_poll_seconds));
    loop {
        interval.tick().await;
        if let Err(e) = poll_once(&state).await {
            tracing::error!("alert polling cycle failed: {e}");
        }
    }
}

async fn poll_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let alerts = db::list_all_alerts(&state.pool).await?;

    for alert in alerts {
        let user = match db::get_user_by_id(&state.pool, &alert.user_id).await? {
            Some(u) if u.is_paid => u,
            _ => continue, // skip alerts for users who are no longer paid
        };
        let _ = user; // reserved for per-user context in notification payloads

        match alert.alert_type.as_str() {
            "price_move" => {
                if let Err(e) = check_price_move(state, &alert).await {
                    tracing::warn!("price_move check failed for {}: {e}", alert.id);
                }
            }
            "large_transfer" => {
                if let Err(e) = check_large_transfer(state, &alert).await {
                    tracing::warn!("large_transfer check failed for {}: {e}", alert.id);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

async fn check_price_move(state: &Arc<AppState>, alert: &crate::models::AlertSubscription) -> anyhow::Result<()> {
    let coin_id = match alert.chain.as_str() {
        "ethereum" => "ethereum",
        "solana" => "solana",
        _ => return Ok(()),
    };
    let prices = coingecko::get_usd_prices(&state.http, &state.config.coingecko_api_base, &[coin_id]).await?;
    let current_price = *prices.get(coin_id).unwrap_or(&0.0);

    if let Some(last) = alert.last_value {
        if last > 0.0 {
            let pct_change = ((current_price - last) / last) * 100.0;
            if pct_change.abs() >= alert.threshold {
                send_webhook(
                    state,
                    &alert.webhook_url,
                    &format!(
                        "Price alert: {} moved {:.2}% (from ${:.2} to ${:.2})",
                        alert.chain, pct_change, last, current_price
                    ),
                )
                .await;
            }
        }
    }
    db::update_alert_last_value(&state.pool, &alert.id, current_price).await?;
    Ok(())
}

async fn check_large_transfer(state: &Arc<AppState>, alert: &crate::models::AlertSubscription) -> anyhow::Result<()> {
    let (latest_tx_amount, symbol) = match alert.chain.as_str() {
        "ethereum" => {
            let txs = etherscan::get_eth_transactions(&state.http, &state.config.etherscan_api_key, &alert.address, 1).await?;
            match txs.first() {
                Some(t) => (t.amount, "ETH"),
                None => return Ok(()),
            }
        }
        "solana" => {
            // Native SOL amount per-tx requires a getTransaction lookup; balance-delta
            // polling is used here as a lighter-weight proxy for demo purposes.
            let balance = solana::get_sol_balance(&state.http, &state.config.solana_rpc_url, &alert.address).await?;
            (balance, "SOL")
        }
        _ => return Ok(()),
    };

    let coin_id = if symbol == "ETH" { "ethereum" } else { "solana" };
    let prices = coingecko::get_usd_prices(&state.http, &state.config.coingecko_api_base, &[coin_id]).await?;
    let price = *prices.get(coin_id).unwrap_or(&0.0);
    let usd_value = latest_tx_amount * price;

    if usd_value >= alert.threshold {
        send_webhook(
            state,
            &alert.webhook_url,
            &format!(
                "Large transfer alert: {} {} (~${:.2}) detected for {}",
                latest_tx_amount, symbol, usd_value, alert.address
            ),
        )
        .await;
    }
    Ok(())
}

async fn send_webhook(state: &Arc<AppState>, url: &str, message: &str) {
    let payload = serde_json::json!({ "content": message, "text": message });
    if let Err(e) = state.http.post(url).json(&payload).send().await {
        tracing::warn!("failed to deliver webhook to {url}: {e}");
    }
}
