use crate::AppState;
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(60);
const MAX_REQUESTS_PER_WINDOW: u32 = 60;

/// Fixed-window rate limiter, keyed by client IP. Kept simple and
/// dependency-free rather than pulling in tower_governor: good enough to
/// stop naive abuse of /api/signup and the wallet lookup endpoints.
///
/// IMPORTANT if deploying behind a reverse proxy (Render, Heroku, etc.):
/// the raw socket peer address will be the proxy's IP for every request,
/// which would rate-limit all users together. This looks for
/// `X-Forwarded-For` first (which Render sets to the real client IP) and
/// only falls back to the socket address for local/direct connections.
pub async fn rate_limit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let ip = client_ip(&req, addr);

    let allowed = {
        let mut map = state.rate_limiter.lock().unwrap();
        let now = Instant::now();
        let entry = map.entry(ip).or_insert((now, 0));

        if now.duration_since(entry.0) > WINDOW {
            *entry = (now, 1);
            true
        } else if entry.1 < MAX_REQUESTS_PER_WINDOW {
            entry.1 += 1;
            true
        } else {
            false
        }
    };

    if !allowed {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    Ok(next.run(req).await)
}

fn client_ip(req: &Request<Body>, fallback: SocketAddr) -> IpAddr {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .and_then(|v| v.trim().parse::<IpAddr>().ok())
        .unwrap_or_else(|| fallback.ip())
}
