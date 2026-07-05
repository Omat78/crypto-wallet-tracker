use crate::models::{PnlSummary, TxRecord};

/// A simplified P&L model:
/// - realized_pnl is approximated as 0 (needs historical cost-basis lot tracking
///   per acquisition, which requires a paid tx-indexing API for accurate results).
/// - unrealized_pnl compares current holdings value against the USD value of
///   native-asset inflows at time of transfer, using the *current* price as a
///   stand-in cost basis proxy.
/// This is intentionally conservative and documented as an approximation —
/// swap in a proper FIFO/LIFO lot tracker (e.g. backed by a tx-indexing API
/// like Covalent, Zerion, or Etherscan + historical price lookups) for
/// production-grade accuracy.
pub fn compute_pnl(
    chain: &str,
    address: &str,
    txs: &[TxRecord],
    current_price_usd: f64,
    current_holdings_native: f64,
) -> PnlSummary {
    let mut inflow_native = 0.0;
    let mut outflow_native = 0.0;

    for tx in txs {
        match tx.direction.as_str() {
            "in" => inflow_native += tx.amount,
            "out" => outflow_native += tx.amount,
            _ => {}
        }
    }

    let total_inflow_usd = inflow_native * current_price_usd;
    let total_outflow_usd = outflow_native * current_price_usd;
    let unrealized_pnl_usd = (current_holdings_native * current_price_usd) - total_inflow_usd + total_outflow_usd;

    PnlSummary {
        chain: chain.to_string(),
        address: address.to_string(),
        realized_pnl_usd: 0.0,
        unrealized_pnl_usd,
        total_inflow_usd,
        total_outflow_usd,
        note: "Approximate P&L using current price as cost-basis proxy. For exact realized gains, integrate historical price-at-transfer-time data.".to_string(),
    }
}
