use crate::handlers::auth::authenticate;
use crate::models::{PnlSummary, TokenHolding, TxRecord, WalletHoldings};
use crate::services::{coingecko, etherscan, pnl, solana};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use std::collections::HashMap;
use std::sync::Arc;

/// Map a small set of well-known symbols to CoinGecko coin ids.
/// Extend this table (or replace with the /coins/list lookup) to support more assets.
fn coingecko_id_for(chain: &str, symbol: &str) -> Option<&'static str> {
    match (chain, symbol.to_uppercase().as_str()) {
        ("ethereum", "ETH") => Some("ethereum"),
        ("solana", "SOL") => Some("solana"),
        (_, "USDC") => Some("usd-coin"),
        (_, "USDT") => Some("tether"),
        (_, "WETH") => Some("weth"),
        (_, "WBTC") => Some("wrapped-bitcoin"),
        _ => None,
    }
}

pub async fn get_holdings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((chain, address)): Path<(String, String)>,
) -> Result<Json<WalletHoldings>, (StatusCode, String)> {
    authenticate(&state, &headers).await?;

    let mut tokens = Vec::new();

    match chain.as_str() {
        "ethereum" => {
            let eth_balance = etherscan::get_eth_balance(&state.http, &state.config.etherscan_api_key, &address)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("etherscan error: {e}")))?;

            // Approximate ERC-20 balances by netting transfer events (in - out).
            let transfers = etherscan::get_erc20_transfers(&state.http, &state.config.etherscan_api_key, &address, 1000)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("etherscan error: {e}")))?;

            let addr_lower = address.to_lowercase();
            let mut net_balances: HashMap<String, f64> = HashMap::new();
            for t in &transfers {
                let symbol = t.get("tokenSymbol").and_then(|v| v.as_str()).unwrap_or("UNKNOWN").to_string();
                let decimals: u32 = t.get("tokenDecimal").and_then(|v| v.as_str()).unwrap_or("18").parse().unwrap_or(18);
                let raw_value: f64 = t.get("value").and_then(|v| v.as_str()).unwrap_or("0").parse().unwrap_or(0.0);
                let amount = raw_value / 10f64.powi(decimals as i32);
                let to = t.get("to").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();

                let entry = net_balances.entry(symbol).or_insert(0.0);
                if to == addr_lower {
                    *entry += amount;
                } else {
                    *entry -= amount;
                }
            }

            let mut ids_to_fetch = vec!["ethereum"];
            let symbol_ids: Vec<(String, &'static str)> = net_balances
                .keys()
                .filter_map(|s| coingecko_id_for("ethereum", s).map(|id| (s.clone(), id)))
                .collect();
            for (_, id) in &symbol_ids {
                ids_to_fetch.push(id);
            }
            ids_to_fetch.sort_unstable();
            ids_to_fetch.dedup();

            let prices = coingecko::get_usd_prices(&state.http, &state.config.coingecko_api_base, &ids_to_fetch)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("coingecko error: {e}")))?;

            let eth_price = *prices.get("ethereum").unwrap_or(&0.0);
            tokens.push(TokenHolding {
                symbol: "ETH".to_string(),
                name: "Ethereum".to_string(),
                balance: eth_balance,
                usd_price: eth_price,
                usd_value: eth_balance * eth_price,
            });

            for (symbol, balance) in net_balances {
                if balance.abs() < f64::EPSILON {
                    continue;
                }
                if let Some(id) = coingecko_id_for("ethereum", &symbol) {
                    let price = *prices.get(id).unwrap_or(&0.0);
                    tokens.push(TokenHolding {
                        symbol: symbol.clone(),
                        name: symbol,
                        balance,
                        usd_price: price,
                        usd_value: balance * price,
                    });
                }
            }
        }
        "solana" => {
            let sol_balance = solana::get_sol_balance(&state.http, &state.config.solana_rpc_url, &address)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("solana rpc error: {e}")))?;

            let spl_holdings = solana::get_spl_token_holdings(&state.http, &state.config.solana_rpc_url, &address)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("solana rpc error: {e}")))?;

            // Only fetch prices for mints we recognize; unrecognized SPL
            // tokens are skipped from display (see solana::known_spl_token).
            let recognized: Vec<(f64, &'static str, &'static str, &'static str)> = spl_holdings
                .into_iter()
                .filter_map(|(mint, amount)| {
                    solana::known_spl_token(&mint).map(|(symbol, name, coingecko_id)| (amount, symbol, name, coingecko_id))
                })
                .collect();

            let mut ids_to_fetch = vec!["solana"];
            for (_, _, _, coingecko_id) in &recognized {
                ids_to_fetch.push(coingecko_id);
            }
            ids_to_fetch.sort_unstable();
            ids_to_fetch.dedup();

            let prices = coingecko::get_usd_prices(&state.http, &state.config.coingecko_api_base, &ids_to_fetch)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("coingecko error: {e}")))?;
            let sol_price = *prices.get("solana").unwrap_or(&0.0);

            tokens.push(TokenHolding {
                symbol: "SOL".to_string(),
                name: "Solana".to_string(),
                balance: sol_balance,
                usd_price: sol_price,
                usd_value: sol_balance * sol_price,
            });

            for (amount, symbol, name, coingecko_id) in recognized {
                let price = *prices.get(coingecko_id).unwrap_or(&0.0);
                tokens.push(TokenHolding {
                    symbol: symbol.to_string(),
                    name: name.to_string(),
                    balance: amount,
                    usd_price: price,
                    usd_value: amount * price,
                });
            }
        }
        _ => return Err((StatusCode::BAD_REQUEST, "unsupported chain: use 'ethereum' or 'solana'".to_string())),
    }

    let total_usd_value = tokens.iter().map(|t| t.usd_value).sum();

    Ok(Json(WalletHoldings {
        chain,
        address,
        tokens,
        total_usd_value,
    }))
}

pub async fn get_transactions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((chain, address)): Path<(String, String)>,
) -> Result<Json<Vec<TxRecord>>, (StatusCode, String)> {
    authenticate(&state, &headers).await?;

    let txs = match chain.as_str() {
        "ethereum" => etherscan::get_eth_transactions(&state.http, &state.config.etherscan_api_key, &address, 100)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, format!("etherscan error: {e}")))?,
        "solana" => solana::get_sol_signatures(&state.http, &state.config.solana_rpc_url, &address, 100)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, format!("solana rpc error: {e}")))?,
        _ => return Err((StatusCode::BAD_REQUEST, "unsupported chain: use 'ethereum' or 'solana'".to_string())),
    };

    Ok(Json(txs))
}

pub async fn get_pnl(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((chain, address)): Path<(String, String)>,
) -> Result<Json<PnlSummary>, (StatusCode, String)> {
    authenticate(&state, &headers).await?;

    let (txs, price, holdings_native) = match chain.as_str() {
        "ethereum" => {
            let txs = etherscan::get_eth_transactions(&state.http, &state.config.etherscan_api_key, &address, 1000)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("etherscan error: {e}")))?;
            let balance = etherscan::get_eth_balance(&state.http, &state.config.etherscan_api_key, &address)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("etherscan error: {e}")))?;
            let prices = coingecko::get_usd_prices(&state.http, &state.config.coingecko_api_base, &["ethereum"])
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("coingecko error: {e}")))?;
            (txs, *prices.get("ethereum").unwrap_or(&0.0), balance)
        }
        "solana" => {
            // Only request as many signatures as will actually get their amount
            // resolved (see solana::AMOUNT_RESOLUTION_LIMIT) — fetching more
            // would just add entries with amount=0 that silently understate
            // inflow/outflow for wallets with long histories.
            let txs = solana::get_sol_signatures(&state.http, &state.config.solana_rpc_url, &address, solana::AMOUNT_RESOLUTION_LIMIT)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("solana rpc error: {e}")))?;
            let balance = solana::get_sol_balance(&state.http, &state.config.solana_rpc_url, &address)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("solana rpc error: {e}")))?;
            let prices = coingecko::get_usd_prices(&state.http, &state.config.coingecko_api_base, &["solana"])
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("coingecko error: {e}")))?;
            (txs, *prices.get("solana").unwrap_or(&0.0), balance)
        }
        _ => return Err((StatusCode::BAD_REQUEST, "unsupported chain: use 'ethereum' or 'solana'".to_string())),
    };

    Ok(Json(pnl::compute_pnl(&chain, &address, &txs, price, holdings_native)))
}
