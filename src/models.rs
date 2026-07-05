use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub is_paid: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertSubscription {
    pub id: String,
    pub user_id: String,
    pub chain: String,
    pub address: String,
    pub alert_type: String, // "price_move" | "large_transfer"
    pub threshold: f64,     // percent for price_move, USD value for large_transfer
    pub webhook_url: String,
    pub last_value: Option<f64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenHolding {
    pub symbol: String,
    pub name: String,
    pub balance: f64,
    pub usd_price: f64,
    pub usd_value: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletHoldings {
    pub chain: String,
    pub address: String,
    pub tokens: Vec<TokenHolding>,
    pub total_usd_value: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TxRecord {
    pub hash: String,
    pub timestamp: DateTime<Utc>,
    pub direction: String, // "in" | "out"
    pub asset: String,
    pub amount: f64,
    pub counterparty: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PnlSummary {
    pub chain: String,
    pub address: String,
    pub realized_pnl_usd: f64,
    pub unrealized_pnl_usd: f64,
    pub total_inflow_usd: f64,
    pub total_outflow_usd: f64,
    pub note: String,
}

#[derive(Debug, Deserialize)]
pub struct SignupRequest {
    pub email: String,
}

#[derive(Debug, Serialize)]
pub struct SignupResponse {
    pub api_key: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateAlertRequest {
    pub chain: String,
    pub address: String,
    pub alert_type: String,
    pub threshold: f64,
    pub webhook_url: String,
}

#[derive(Debug, Deserialize)]
pub struct RecoverRequest {
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiKeyQuery {
    pub api_key: Option<String>,
}
