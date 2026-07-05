use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub port: u16,
    pub database_url: String,
    pub etherscan_api_key: String,
    pub coingecko_api_base: String,
    pub solana_rpc_url: String,
    pub alert_poll_seconds: u64,
    pub stripe_webhook_secret: String,
    pub allowed_origins: Vec<String>,
    /// Base URL this app is publicly reachable at (used to build Stripe
    /// Checkout success/cancel redirect URLs). e.g. https://yourapp.onrender.com
    pub app_base_url: String,
    /// Stripe secret key (starts with sk_...), used server-side to create
    /// Checkout Sessions via the REST API. Never sent to the browser.
    pub stripe_secret_key: String,
    /// The Stripe Price ID (starts with price_...) for the paid alerts subscription.
    pub stripe_price_id: String,
    /// Resend.com API key, used to email recovered API keys. If unset, the
    /// recovered key is logged server-side instead (dev-only fallback).
    pub resend_api_key: String,
    /// "From" address for recovery emails — must be a verified sender/domain
    /// in your Resend account.
    pub email_from: String,
}

impl Config {
    pub fn from_env() -> Self {
        let port: u16 = env::var("PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(8080);
        Self {
            port,
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data.db".into()),
            etherscan_api_key: env::var("ETHERSCAN_API_KEY").unwrap_or_default(),
            coingecko_api_base: env::var("COINGECKO_API_BASE")
                .unwrap_or_else(|_| "https://api.coingecko.com/api/v3".into()),
            solana_rpc_url: env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".into()),
            alert_poll_seconds: env::var("ALERT_POLL_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            stripe_webhook_secret: env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_default(),
            allowed_origins: env::var("ALLOWED_ORIGINS")
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            app_base_url: env::var("APP_BASE_URL").unwrap_or_else(|_| format!("http://localhost:{port}")),
            stripe_secret_key: env::var("STRIPE_SECRET_KEY").unwrap_or_default(),
            stripe_price_id: env::var("STRIPE_PRICE_ID").unwrap_or_default(),
            resend_api_key: env::var("RESEND_API_KEY").unwrap_or_default(),
            email_from: env::var("EMAIL_FROM").unwrap_or_else(|_| "Ledger <onboarding@resend.dev>".into()),
        }
    }
}
