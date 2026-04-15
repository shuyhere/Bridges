use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, post};
use axum::{Extension, Json, Router};
use base64::Engine as _;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use super::auth::{AuthUser, SessionAuth};
use super::ServerState;

#[derive(Deserialize)]
pub struct CreateTokenReq {
    pub name: String,
    pub scopes: Option<String>,
    /// Token lifetime in seconds. Null = no expiry.
    #[serde(rename = "expiresIn")]
    pub expires_in: Option<i64>,
}

#[derive(Serialize)]
pub struct CreateTokenResp {
    #[serde(rename = "tokenId")]
    pub token_id: String,
    /// The plaintext API token — only shown once at creation.
    pub token: String,
    pub name: String,
    pub scopes: String,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Serialize)]
pub struct TokenListItem {
    #[serde(rename = "tokenId")]
    pub token_id: String,
    pub name: String,
    pub scopes: String,
    /// Prefix of the token for identification (e.g., "bridges_sk_Ab3...").
    pub prefix: String,
    #[serde(rename = "lastUsedAt", skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Serialize)]
pub struct MessageResp {
    pub ok: bool,
    pub message: String,
}

/// Generate a random API token: `bridges_sk_` + 32 random bytes base64url.
fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    format!("bridges_sk_{}", encoded)
}

fn hash_token(key: &str) -> String {
    let mut h = Sha256::new();
    h.update(key.as_bytes());
    hex::encode(h.finalize())
}

pub fn routes(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/v1/tokens", post(create_token).get(list_tokens))
        .route("/v1/tokens/:id", delete(delete_token))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            SessionAuth::middleware,
        ))
        .with_state(state)
}

async fn create_token(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<CreateTokenReq>,
) -> Result<Json<CreateTokenResp>, (StatusCode, Json<MessageResp>)> {
    if req.name.is_empty() || req.name.len() > 64 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(MessageResp {
                ok: false,
                message: "Token name must be 1-64 characters".to_string(),
            }),
        ));
    }

    let token = generate_token();
    let token_hash = hash_token(&token);
    let token_id = format!("tok_{}", uuid::Uuid::new_v4());
    let now = chrono::Utc::now();
    let now_str = now.to_rfc3339();
    let scopes = req.scopes.unwrap_or_else(|| "all".to_string());
    let prefix = format!("{}...", &token[..16]);

    let expires_at = req
        .expires_in
        .map(|secs| (now + chrono::Duration::seconds(secs)).to_rfc3339());

    let db = state.db.lock().await;

    // Limit tokens per user (max 20)
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM user_tokens WHERE user_id = ?1",
            rusqlite::params![auth.user_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if count >= 20 {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(MessageResp {
                ok: false,
                message: "Maximum 20 tokens per user".to_string(),
            }),
        ));
    }

    db.execute(
        "INSERT INTO user_tokens (token_id, user_id, token_hash, name, scopes, prefix, expires_at, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![token_id, auth.user_id, token_hash, req.name, scopes, prefix, expires_at, now_str],
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MessageResp {
                ok: false,
                message: format!("Failed to create token: {}", e),
            }),
        )
    })?;

    Ok(Json(CreateTokenResp {
        token_id,
        token,
        name: req.name,
        scopes,
        expires_at,
        created_at: now_str,
    }))
}

async fn list_tokens(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Vec<TokenListItem>>, StatusCode> {
    let db = state.db.lock().await;
    let mut stmt = db
        .prepare(
            "SELECT token_id, name, scopes, prefix, last_used_at, expires_at, created_at \
             FROM user_tokens WHERE user_id = ?1 ORDER BY created_at DESC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let tokens = stmt
        .query_map(rusqlite::params![auth.user_id], |row| {
            Ok(TokenListItem {
                token_id: row.get(0)?,
                name: row.get(1)?,
                scopes: row.get(2)?,
                prefix: row.get(3)?,
                last_used_at: row.get(4)?,
                expires_at: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(tokens))
}

async fn delete_token(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthUser>,
    Path(token_id): Path<String>,
) -> Result<Json<MessageResp>, StatusCode> {
    let db = state.db.lock().await;
    let affected = db
        .execute(
            "DELETE FROM user_tokens WHERE token_id = ?1 AND user_id = ?2",
            rusqlite::params![token_id, auth.user_id],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if affected == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(MessageResp {
        ok: true,
        message: "Token revoked".to_string(),
    }))
}
