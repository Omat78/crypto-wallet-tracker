use crate::db;
use crate::models::{SignupRequest, SignupResponse, User};
use crate::security;
use crate::AppState;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use std::sync::Arc;

pub async fn signup(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SignupRequest>,
) -> Result<Json<SignupResponse>, (StatusCode, String)> {
    let (_user, raw_key) = db::create_user(&state.pool, &req.email)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("could not create user: {e}")))?;

    Ok(Json(SignupResponse {
        api_key: raw_key,
        message: "Save this API key — it will not be shown again. Pass it as the `x-api-key` header on requests.".to_string(),
    }))
}

/// Regenerates an API key for the given email and emails it (via Resend —
/// see `services::email`). Always returns the same generic message whether
/// or not an account exists for that email, to avoid leaking which emails
/// are registered. The previous key for that account stops working
/// immediately once a new one is issued.
pub async fn recover(
    State(state): State<Arc<AppState>>,
    Json(req): Json<crate::models::RecoverRequest>,
) -> Json<serde_json::Value> {
    match db::regenerate_key_for_email(&state.pool, &req.email).await {
        Ok(Some(raw_key)) => {
            if let Err(e) = crate::services::email::send_key_recovery_email(
                &state.http,
                &state.config.resend_api_key,
                &state.config.email_from,
                &req.email,
                &raw_key,
            )
            .await
            {
                tracing::error!("failed to send recovery email to {}: {e}", req.email);
            }
        }
        Ok(None) => { /* no account for this email — respond identically either way */ }
        Err(e) => tracing::error!("key recovery lookup failed: {e}"),
    }

    Json(serde_json::json!({
        "message": "If that email has an account, a new API key has been sent to it. Any previous key for that account is now inactive."
    }))
}

pub async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let user = authenticate(&state, &headers).await?;
    Ok(Json(serde_json::json!({ "email": user.email, "is_paid": user.is_paid })))
}

/// Resolve the authenticated user from the `x-api-key` header.
pub async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<User, (StatusCode, String)> {
    let api_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .ok_or((StatusCode::UNAUTHORIZED, "missing x-api-key header".to_string()))?;

    db::get_user_by_api_key(&state.pool, api_key)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::UNAUTHORIZED, "invalid api key".to_string()))
}

/// Stripe webhook: verifies the `Stripe-Signature` header against the raw
/// request body (see `security::verify_stripe_signature`) before trusting
/// the payload. Marks a user as paid when a checkout.session.completed
/// event arrives for their email.
pub async fn stripe_webhook(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> StatusCode {
    if state.config.stripe_webhook_secret.is_empty() {
        tracing::error!("STRIPE_WEBHOOK_SECRET is not set — refusing to process webhook");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    let sig_header = match headers.get("stripe-signature").and_then(|v| v.to_str().ok()) {
        Some(s) => s,
        None => return StatusCode::BAD_REQUEST,
    };

    if let Err(e) = security::verify_stripe_signature(&body, sig_header, &state.config.stripe_webhook_secret) {
        tracing::warn!("rejected stripe webhook: {e}");
        return StatusCode::BAD_REQUEST;
    }

    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return StatusCode::BAD_REQUEST,
    };

    let event_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if event_type == "checkout.session.completed" {
        if let Some(email) = payload
            .pointer("/data/object/customer_details/email")
            .and_then(|v| v.as_str())
        {
            if let Err(e) = db::mark_user_paid(&state.pool, email).await {
                tracing::error!("failed to mark user paid: {e}");
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
        }
    }
    StatusCode::OK
}
