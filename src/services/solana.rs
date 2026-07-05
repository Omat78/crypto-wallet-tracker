use crate::models::TxRecord;
use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};
use serde_json::json;

/// Only the most recent N signatures get a follow-up `getTransaction` call
/// to resolve their exact amount — resolving all of them would mean one RPC
/// call per transaction, which is slow and easy to rate-limit against on
/// the free public Solana RPC endpoint. Older transactions in the list keep
/// direction/amount as "unknown"/0.
pub const AMOUNT_RESOLUTION_LIMIT: usize = 20;

const SPL_TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// Native SOL balance, denominated in SOL (not lamports).
pub async fn get_sol_balance(client: &reqwest::Client, rpc_url: &str, address: &str) -> Result<f64> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBalance",
        "params": [address]
    });
    let resp: serde_json::Value = client.post(rpc_url).json(&body).send().await?.json().await?;
    let lamports = resp
        .get("result")
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow!("unexpected solana rpc response: {resp}"))?;
    Ok(lamports as f64 / 1e9)
}

/// Recent confirmed signatures (transaction history) for an address, with
/// exact SOL amount/direction resolved for the most recent
/// `AMOUNT_RESOLUTION_LIMIT` of them via a follow-up `getTransaction` call each.
pub async fn get_sol_signatures(
    client: &reqwest::Client,
    rpc_url: &str,
    address: &str,
    limit: usize,
) -> Result<Vec<TxRecord>> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignaturesForAddress",
        "params": [address, { "limit": limit }]
    });
    let resp: serde_json::Value = client.post(rpc_url).json(&body).send().await?.json().await?;
    let items = resp
        .get("result")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::new();
    for (i, item) in items.into_iter().enumerate() {
        let sig = item.get("signature").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let block_time = item.get("blockTime").and_then(|v| v.as_i64()).unwrap_or(0);
        let err_is_null = item.get("err").map(|v| v.is_null()).unwrap_or(true);

        let (direction, amount) = if err_is_null && i < AMOUNT_RESOLUTION_LIMIT {
            match get_transaction_amount(client, rpc_url, &sig, address).await {
                Ok(Some((dir, amt))) => (dir, amt),
                _ => ("unknown".to_string(), 0.0),
            }
        } else if !err_is_null {
            ("failed".to_string(), 0.0)
        } else {
            ("unknown".to_string(), 0.0)
        };

        out.push(TxRecord {
            hash: sig,
            timestamp: Utc.timestamp_opt(block_time, 0).single().unwrap_or_else(Utc::now),
            direction,
            asset: "SOL".to_string(),
            amount,
            counterparty: "".to_string(),
        });
    }
    Ok(out)
}

/// Resolves the exact native-SOL balance change for `wallet_address` in a
/// given transaction, by diffing `preBalances`/`postBalances` at that
/// account's index. Returns `("in", amount)` / `("out", amount)` where
/// `amount` is always positive, or `None` if the transaction couldn't be
/// parsed (e.g. an unsupported version).
async fn get_transaction_amount(
    client: &reqwest::Client,
    rpc_url: &str,
    signature: &str,
    wallet_address: &str,
) -> Result<Option<(String, f64)>> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTransaction",
        "params": [signature, { "encoding": "jsonParsed", "maxSupportedTransactionVersion": 0 }]
    });
    let resp: serde_json::Value = client.post(rpc_url).json(&body).send().await?.json().await?;

    let result = match resp.get("result") {
        Some(r) if !r.is_null() => r,
        _ => return Ok(None),
    };

    let account_keys = result
        .pointer("/transaction/message/accountKeys")
        .and_then(|v| v.as_array());
    let pre_balances = result.pointer("/meta/preBalances").and_then(|v| v.as_array());
    let post_balances = result.pointer("/meta/postBalances").and_then(|v| v.as_array());

    let (Some(keys), Some(pre), Some(post)) = (account_keys, pre_balances, post_balances) else {
        return Ok(None);
    };

    let idx = keys.iter().position(|k| {
        k.get("pubkey").and_then(|v| v.as_str()) == Some(wallet_address)
    });

    let Some(idx) = idx else { return Ok(None) };

    let pre_lamports = pre.get(idx).and_then(|v| v.as_i64()).unwrap_or(0);
    let post_lamports = post.get(idx).and_then(|v| v.as_i64()).unwrap_or(0);
    let delta = post_lamports - pre_lamports;

    let direction = if delta >= 0 { "in" } else { "out" };
    let amount = (delta.abs() as f64) / 1e9;

    Ok(Some((direction.to_string(), amount)))
}

/// Returns (symbol, name, coingecko_id) for a small set of well-known SPL
/// token mints. Unrecognized mints are skipped from holdings display rather
/// than shown with a placeholder — extend this table (or swap in the Jupiter
/// token list API) to support more tokens.
pub fn known_spl_token(mint: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match mint {
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" => Some(("USDC", "USD Coin", "usd-coin")),
        "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" => Some(("USDT", "Tether", "tether")),
        "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So" => Some(("mSOL", "Marinade staked SOL", "msol")),
        _ => None,
    }
}

/// SPL token balances for an address, as (mint, ui_amount) pairs — only
/// non-zero balances are returned. Uses `getTokenAccountsByOwner` with
/// jsonParsed encoding so amounts come back already decimal-adjusted.
pub async fn get_spl_token_holdings(
    client: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> Result<Vec<(String, f64)>> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTokenAccountsByOwner",
        "params": [
            address,
            { "programId": SPL_TOKEN_PROGRAM_ID },
            { "encoding": "jsonParsed" }
        ]
    });
    let resp: serde_json::Value = client.post(rpc_url).json(&body).send().await?.json().await?;

    let accounts = resp
        .pointer("/result/value")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::new();
    for acct in accounts {
        let info = acct.pointer("/account/data/parsed/info");
        let mint = info.and_then(|v| v.get("mint")).and_then(|v| v.as_str());
        let ui_amount = info
            .and_then(|v| v.pointer("/tokenAmount/uiAmount"))
            .and_then(|v| v.as_f64());

        if let (Some(mint), Some(amount)) = (mint, ui_amount) {
            if amount > 0.0 {
                out.push((mint.to_string(), amount));
            }
        }
    }
    Ok(out)
}
