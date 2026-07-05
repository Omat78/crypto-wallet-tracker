use anyhow::{anyhow, Context, Result};

/// Creates a Stripe Checkout Session for the paid alerts subscription and
/// returns the hosted checkout URL to redirect the user to.
///
/// Calls Stripe's REST API directly (form-encoded, like their own curl
/// examples) rather than pulling in the full `stripe` crate — keeps the
/// dependency tree (and your build times) small.
pub async fn create_checkout_session(
    client: &reqwest::Client,
    secret_key: &str,
    price_id: &str,
    customer_email: &str,
    success_url: &str,
    cancel_url: &str,
) -> Result<String> {
    if secret_key.is_empty() || price_id.is_empty() {
        return Err(anyhow!("Stripe is not configured (STRIPE_SECRET_KEY / STRIPE_PRICE_ID missing)"));
    }

    let params = [
        ("mode", "subscription"),
        ("line_items[0][price]", price_id),
        ("line_items[0][quantity]", "1"),
        ("customer_email", customer_email),
        ("success_url", success_url),
        ("cancel_url", cancel_url),
    ];

    let resp: serde_json::Value = client
        .post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(secret_key, Some(""))
        .form(&params)
        .send()
        .await
        .context("failed to reach Stripe API")?
        .json()
        .await
        .context("Stripe response was not valid JSON")?;

    if let Some(err) = resp.get("error") {
        let message = err.get("message").and_then(|v| v.as_str()).unwrap_or("unknown Stripe error");
        return Err(anyhow!("Stripe error: {message}"));
    }

    resp.get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Stripe response had no checkout url: {resp}"))
}
