use anyhow::{Context, Result};
use serde_json::json;

/// Sends a recovered API key to the account's email via Resend
/// (https://resend.com — a simple REST email API, no SMTP setup needed).
///
/// If `resend_api_key` is empty, this logs the key server-side instead of
/// emailing it. That's a deliberate dev-mode fallback so the recovery flow
/// still works before you've configured a real email provider — but it
/// means anyone with server log access could see it, so don't rely on this
/// fallback in production.
pub async fn send_key_recovery_email(
    client: &reqwest::Client,
    resend_api_key: &str,
    email_from: &str,
    to_email: &str,
    raw_key: &str,
) -> Result<()> {
    if resend_api_key.is_empty() {
        tracing::warn!(
            "RESEND_API_KEY not set — logging the recovered key instead of emailing it. \
             Recovered key for {to_email}: {raw_key}"
        );
        return Ok(());
    }

    let body = json!({
        "from": email_from,
        "to": [to_email],
        "subject": "Your new Ledger API key",
        "text": format!(
            "Here is your new API key:\n\n{raw_key}\n\n\
             Your previous key has been deactivated. If you didn't request this, \
             you can ignore this email — your account is still safe."
        ),
    });

    client
        .post("https://api.resend.com/emails")
        .bearer_auth(resend_api_key)
        .json(&body)
        .send()
        .await
        .context("failed to reach Resend API")?
        .error_for_status()
        .context("Resend API returned an error")?;

    Ok(())
}
