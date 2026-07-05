use crate::handlers::auth::authenticate;
use crate::services::stripe;
use crate::AppState;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use std::sync::Arc;

/// Creates a Stripe Checkout Session for the authenticated user and returns
/// the hosted checkout URL for the browser to redirect to. On completion,
/// Stripe calls `/api/stripe/webhook`, which is what actually flips the
/// user to paid (see handlers::auth::stripe_webhook) — this endpoint only
/// starts the checkout flow.
pub async fn create_checkout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let user = authenticate(&state, &headers).await?;

    if user.is_paid {
        return Err((StatusCode::BAD_REQUEST, "This account is already on the paid tier.".to_string()));
    }

    let success_url = format!("{}/?checkout=success", state.config.app_base_url);
    let cancel_url = format!("{}/?checkout=cancelled", state.config.app_base_url);

    let checkout_url = stripe::create_checkout_session(
        &state.http,
        &state.config.stripe_secret_key,
        &state.config.stripe_price_id,
        &user.email,
        &success_url,
        &cancel_url,
    )
    .await
    .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    Ok(Json(serde_json::json!({ "checkout_url": checkout_url })))
}
