use crate::db;
use crate::handlers::auth::authenticate;
use crate::models::{AlertSubscription, CreateAlertRequest};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use std::sync::Arc;

pub async fn create_alert(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateAlertRequest>,
) -> Result<Json<AlertSubscription>, (StatusCode, String)> {
    let user = authenticate(&state, &headers).await?;

    if !user.is_paid {
        return Err((
            StatusCode::PAYMENT_REQUIRED,
            "Real-time alerts are a paid feature. Upgrade to subscribe to price-move and large-transfer alerts.".to_string(),
        ));
    }

    if !["price_move", "large_transfer"].contains(&req.alert_type.as_str()) {
        return Err((StatusCode::BAD_REQUEST, "alert_type must be 'price_move' or 'large_transfer'".to_string()));
    }

    let alert = db::create_alert(&state.pool, &user.id, &req)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(alert))
}

pub async fn list_alerts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<AlertSubscription>>, (StatusCode, String)> {
    let user = authenticate(&state, &headers).await?;
    let alerts = db::list_alerts_for_user(&state.pool, &user.id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(alerts))
}

pub async fn delete_alert(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let user = authenticate(&state, &headers).await?;
    let deleted = db::delete_alert(&state.pool, &id, &user.id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if deleted == 0 {
        return Err((StatusCode::NOT_FOUND, "alert not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}
