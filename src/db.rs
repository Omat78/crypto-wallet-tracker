use crate::models::{AlertSubscription, User};
use anyhow::Result;
use chrono::Utc;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use uuid::Uuid;

pub async fn init_pool(database_url: &str) -> Result<SqlitePool> {
    // Ensure the sqlite file can be created if it doesn't exist.
    let url = if database_url.contains("?") {
        database_url.to_string()
    } else {
        format!("{}?mode=rwc", database_url)
    };
    let pool = SqlitePoolOptions::new().max_connections(5).connect(&url).await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            email TEXT UNIQUE NOT NULL,
            api_key_hash TEXT UNIQUE NOT NULL,
            is_paid INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS alert_subscriptions (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            chain TEXT NOT NULL,
            address TEXT NOT NULL,
            alert_type TEXT NOT NULL,
            threshold REAL NOT NULL,
            webhook_url TEXT NOT NULL,
            last_value REAL,
            created_at TEXT NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

/// Creates a user and returns both the stored `User` and the raw API key.
/// The raw key is never persisted — only its SHA-256 hash is stored — so it
/// must be shown to the caller immediately and cannot be recovered later.
pub async fn create_user(pool: &SqlitePool, email: &str) -> Result<(User, String)> {
    let raw_key = format!("cwt_{}", Uuid::new_v4().simple());
    let key_hash = crate::security::sha256_hex(&raw_key);

    let user = User {
        id: Uuid::new_v4().to_string(),
        email: email.to_string(),
        is_paid: false,
        created_at: Utc::now(),
    };

    sqlx::query(
        "INSERT INTO users (id, email, api_key_hash, is_paid, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&user.id)
    .bind(&user.email)
    .bind(&key_hash)
    .bind(user.is_paid as i32)
    .bind(user.created_at.to_rfc3339())
    .execute(pool)
    .await?;

    Ok((user, raw_key))
}

pub async fn get_user_by_api_key(pool: &SqlitePool, api_key: &str) -> Result<Option<User>> {
    let key_hash = crate::security::sha256_hex(api_key);
    let row = sqlx::query("SELECT id, email, is_paid, created_at FROM users WHERE api_key_hash = ?")
        .bind(&key_hash)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(|r| User {
        id: r.get("id"),
        email: r.get("email"),
        is_paid: r.get::<i32, _>("is_paid") != 0,
        created_at: chrono::DateTime::parse_from_rfc3339(r.get::<String, _>("created_at").as_str())
            .unwrap()
            .with_timezone(&Utc),
    }))
}

pub async fn mark_user_paid(pool: &SqlitePool, email: &str) -> Result<()> {
    sqlx::query("UPDATE users SET is_paid = 1 WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await?;
    Ok(())
}

/// Generates a new API key for the given email, invalidating any previous
/// key for that account, and returns the new raw key. Returns `Ok(None)` if
/// no account exists for that email — callers should respond identically
/// either way, to avoid leaking which emails have accounts.
pub async fn regenerate_key_for_email(pool: &SqlitePool, email: &str) -> Result<Option<String>> {
    let raw_key = format!("cwt_{}", Uuid::new_v4().simple());
    let key_hash = crate::security::sha256_hex(&raw_key);

    let result = sqlx::query("UPDATE users SET api_key_hash = ? WHERE email = ?")
        .bind(&key_hash)
        .bind(email)
        .execute(pool)
        .await?;

    if result.rows_affected() > 0 {
        Ok(Some(raw_key))
    } else {
        Ok(None)
    }
}

pub async fn create_alert(pool: &SqlitePool, user_id: &str, req: &crate::models::CreateAlertRequest) -> Result<AlertSubscription> {
    let alert = AlertSubscription {
        id: Uuid::new_v4().to_string(),
        user_id: user_id.to_string(),
        chain: req.chain.clone(),
        address: req.address.clone(),
        alert_type: req.alert_type.clone(),
        threshold: req.threshold,
        webhook_url: req.webhook_url.clone(),
        last_value: None,
        created_at: Utc::now(),
    };

    sqlx::query(
        "INSERT INTO alert_subscriptions (id, user_id, chain, address, alert_type, threshold, webhook_url, last_value, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&alert.id)
    .bind(&alert.user_id)
    .bind(&alert.chain)
    .bind(&alert.address)
    .bind(&alert.alert_type)
    .bind(alert.threshold)
    .bind(&alert.webhook_url)
    .bind(alert.last_value)
    .bind(alert.created_at.to_rfc3339())
    .execute(pool)
    .await?;

    Ok(alert)
}

pub async fn list_alerts_for_user(pool: &SqlitePool, user_id: &str) -> Result<Vec<AlertSubscription>> {
    let rows = sqlx::query(
        "SELECT id, user_id, chain, address, alert_type, threshold, webhook_url, last_value, created_at FROM alert_subscriptions WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| AlertSubscription {
            id: r.get("id"),
            user_id: r.get("user_id"),
            chain: r.get("chain"),
            address: r.get("address"),
            alert_type: r.get("alert_type"),
            threshold: r.get("threshold"),
            webhook_url: r.get("webhook_url"),
            last_value: r.get("last_value"),
            created_at: chrono::DateTime::parse_from_rfc3339(r.get::<String, _>("created_at").as_str())
                .unwrap()
                .with_timezone(&Utc),
        })
        .collect())
}

pub async fn list_all_alerts(pool: &SqlitePool) -> Result<Vec<AlertSubscription>> {
    let rows = sqlx::query(
        "SELECT id, user_id, chain, address, alert_type, threshold, webhook_url, last_value, created_at FROM alert_subscriptions",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| AlertSubscription {
            id: r.get("id"),
            user_id: r.get("user_id"),
            chain: r.get("chain"),
            address: r.get("address"),
            alert_type: r.get("alert_type"),
            threshold: r.get("threshold"),
            webhook_url: r.get("webhook_url"),
            last_value: r.get("last_value"),
            created_at: chrono::DateTime::parse_from_rfc3339(r.get::<String, _>("created_at").as_str())
                .unwrap()
                .with_timezone(&Utc),
        })
        .collect())
}

pub async fn update_alert_last_value(pool: &SqlitePool, id: &str, value: f64) -> Result<()> {
    sqlx::query("UPDATE alert_subscriptions SET last_value = ? WHERE id = ?")
        .bind(value)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_alert(pool: &SqlitePool, id: &str, user_id: &str) -> Result<u64> {
    let res = sqlx::query("DELETE FROM alert_subscriptions WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

pub async fn get_user_by_id(pool: &SqlitePool, id: &str) -> Result<Option<User>> {
    let row = sqlx::query("SELECT id, email, is_paid, created_at FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(|r| User {
        id: r.get("id"),
        email: r.get("email"),
        is_paid: r.get::<i32, _>("is_paid") != 0,
        created_at: chrono::DateTime::parse_from_rfc3339(r.get::<String, _>("created_at").as_str())
            .unwrap()
            .with_timezone(&Utc),
    }))
}
