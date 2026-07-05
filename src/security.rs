use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// One-way hash for API keys. Keys are generated server-side with high
/// entropy (a UUID), so a plain SHA-256 digest (no per-user salt) is enough —
/// unlike passwords, there's no risk of dictionary/rainbow-table attacks on
/// a random 128-bit token. Only the hash is ever stored; the raw key is
/// shown to the user exactly once, at signup.
pub fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

/// Verify a Stripe webhook signature per Stripe's signing scheme:
/// https://docs.stripe.com/webhooks#verify-manually
///
/// The `Stripe-Signature` header looks like:
///   t=1614556800,v1=5257a869e7...,v0=...
/// We recompute HMAC-SHA256("{t}.{raw_body}", secret) and compare it
/// (constant-time) against the v1 value, and reject stale timestamps to
/// prevent replay of a captured request.
pub fn verify_stripe_signature(payload: &[u8], sig_header: &str, secret: &str) -> Result<()> {
    if secret.is_empty() {
        return Err(anyhow!("stripe webhook secret is not configured"));
    }

    let mut timestamp: Option<i64> = None;
    let mut v1_sigs: Vec<Vec<u8>> = Vec::new();

    for part in sig_header.split(',') {
        let mut kv = part.splitn(2, '=');
        match (kv.next(), kv.next()) {
            (Some("t"), Some(v)) => timestamp = v.parse().ok(),
            (Some("v1"), Some(v)) => {
                if let Ok(bytes) = hex::decode(v) {
                    v1_sigs.push(bytes);
                }
            }
            _ => {}
        }
    }

    let timestamp = timestamp.ok_or_else(|| anyhow!("missing timestamp in Stripe-Signature header"))?;
    if v1_sigs.is_empty() {
        return Err(anyhow!("no v1 signature found in Stripe-Signature header"));
    }

    // Reject requests whose timestamp is more than 5 minutes off — this
    // prevents a captured, still-validly-signed request from being replayed
    // long after the fact.
    let now = chrono::Utc::now().timestamp();
    if (now - timestamp).abs() > 300 {
        return Err(anyhow!("stripe webhook timestamp is outside the allowed tolerance"));
    }

    let signed_payload = [timestamp.to_string().as_bytes(), b".", payload].concat();

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| anyhow!("invalid webhook secret: {e}"))?;
    mac.update(&signed_payload);

    for candidate in &v1_sigs {
        if mac.clone().verify_slice(candidate).is_ok() {
            return Ok(());
        }
    }

    Err(anyhow!("stripe signature verification failed"))
}
