use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine as _;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use super::ServerState;

/// Extension: authenticated node (from node API key or externally provisioned user token).
#[derive(Clone, Debug)]
pub struct AuthNode(pub String);

/// Generate a random API key: `bridges_sk_` + 32 random bytes base64url.
fn generate_api_key() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    format!("bridges_sk_{}", encoded)
}

fn hash_api_key(key: &str) -> String {
    let mut h = Sha256::new();
    h.update(key.as_bytes());
    hex::encode(h.finalize())
}

#[derive(Deserialize)]
pub struct RegisterReq {
    #[serde(rename = "nodeId")]
    pub node_id: String,
    #[serde(rename = "ed25519Pubkey")]
    pub ed25519_pubkey: String,
    #[serde(rename = "x25519Pubkey")]
    pub x25519_pubkey: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "ownerName")]
    pub owner_name: Option<String>,
}

#[derive(Serialize)]
pub struct RegisterResp {
    #[serde(rename = "apiKey")]
    pub api_key: String,
    #[serde(rename = "nodeId")]
    pub node_id: String,
    #[serde(rename = "giteaUrl", skip_serializing_if = "Option::is_none")]
    pub gitea_url: Option<String>,
    #[serde(rename = "giteaUser", skip_serializing_if = "Option::is_none")]
    pub gitea_user: Option<String>,
    #[serde(rename = "giteaToken", skip_serializing_if = "Option::is_none")]
    pub gitea_token: Option<String>,
    #[serde(rename = "giteaPassword", skip_serializing_if = "Option::is_none")]
    pub gitea_password: Option<String>,
}

pub fn routes(state: Arc<ServerState>) -> Router {
    let public = Router::new()
        .route("/v1/auth/register", post(register))
        .with_state(state.clone());

    let protected = Router::new()
        .route("/v1/auth/refresh", post(refresh))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state);

    public.merge(protected)
}

async fn register(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(req): Json<RegisterReq>,
) -> Result<Json<RegisterResp>, StatusCode> {
    let api_key = generate_api_key();
    let key_hash = hash_api_key(&api_key);
    let now = chrono::Utc::now().to_rfc3339();

    // If the caller supplied an externally provisioned user token, link this node to that user.
    let user_id = resolve_user_from_bearer(&state, &headers).await;

    let (gitea_url, gitea_user, gitea_token, gitea_password) = if let Some(ref gitea) = state.gitea
    {
        match create_gitea_user(gitea, &req.node_id, req.display_name.as_deref()).await {
            Ok((user, token, password)) => {
                let client_url = gitea
                    .external_url
                    .clone()
                    .unwrap_or_else(|| gitea.gitea_url.clone());
                (Some(client_url), Some(user), Some(token), Some(password))
            }
            Err(e) => {
                eprintln!("Gitea account creation failed: {} (continuing without)", e);
                (None, None, None, None)
            }
        }
    } else {
        (None, None, None, None)
    };

    let db = state.db.lock().await;
    db.execute(
        "INSERT OR REPLACE INTO registered_nodes \
         (node_id, ed25519_pubkey, x25519_pubkey, display_name, owner_name, gitea_user, api_key_hash, user_id, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            req.node_id,
            req.ed25519_pubkey,
            req.x25519_pubkey,
            req.display_name,
            req.owner_name,
            gitea_user,
            key_hash,
            user_id,
            now,
        ],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(RegisterResp {
        api_key,
        node_id: req.node_id,
        gitea_url,
        gitea_user,
        gitea_token,
        gitea_password,
    }))
}

/// Create a Gitea user account and generate a personal access token.
async fn create_gitea_user(
    gitea: &super::GiteaConfig,
    node_id: &str,
    display_name: Option<&str>,
) -> Result<(String, String, String), String> {
    let client = reqwest::Client::new();
    let gitea_url = &gitea.gitea_url;

    let username = if let Some(name) = display_name {
        name.to_lowercase()
            .replace(' ', "-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .take(20)
            .collect::<String>()
    } else {
        node_id
            .trim_start_matches("kd_")
            .chars()
            .take(20)
            .collect::<String>()
            .to_lowercase()
    };
    let full_name = display_name.unwrap_or(&username);
    let password = format!("bridges_{}", uuid::Uuid::new_v4());
    let email = format!("{}@bridges.local", username);

    let body = serde_json::json!({
        "username": username,
        "password": password,
        "email": email,
        "full_name": full_name,
        "must_change_password": false,
    });
    let resp = client
        .post(format!("{}/api/v1/admin/users", gitea_url))
        .header("Authorization", format!("token {}", gitea.admin_token))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("gitea create user: {}", e))?;

    if !resp.status().is_success() && resp.status().as_u16() != 422 {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !text.contains("already exists") {
            return Err(format!("gitea create user HTTP {} — {}", status, text));
        }
    }

    let token_body = serde_json::json!({
        "name": format!("bridges-{}", chrono::Utc::now().timestamp()),
        "scopes": ["all"]
    });
    let token_resp = client
        .post(format!("{}/api/v1/users/{}/tokens", gitea_url, username))
        .basic_auth(&username, Some(&password))
        .json(&token_body)
        .send()
        .await
        .map_err(|e| format!("gitea create token: {}", e))?;

    let token_resp = if !token_resp.status().is_success() {
        let mut request = client
            .post(format!("{}/api/v1/users/{}/tokens", gitea_url, username))
            .json(&token_body);
        if let Some(admin_password) = gitea.admin_password.as_deref() {
            request = request.basic_auth(&gitea.admin_user, Some(admin_password));
        } else {
            request = request.header("Authorization", format!("token {}", gitea.admin_token));
        }
        let resp = request
            .send()
            .await
            .map_err(|e| format!("gitea create token (admin): {}", e))?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("gitea create token failed: {}", text));
        }
        resp
    } else {
        token_resp
    };

    let token_val: serde_json::Value = token_resp
        .json()
        .await
        .map_err(|e| format!("parse gitea token: {}", e))?;
    let token = token_val["sha1"]
        .as_str()
        .ok_or_else(|| "no token in gitea response".to_string())?
        .to_string();

    Ok((username, token, password))
}

async fn refresh(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<RegisterResp>, StatusCode> {
    let node_id = extract_node_id(&state, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let api_key = generate_api_key();
    let key_hash = hash_api_key(&api_key);

    let db = state.db.lock().await;
    db.execute(
        "UPDATE registered_nodes SET api_key_hash = ?1 WHERE node_id = ?2",
        rusqlite::params![key_hash, node_id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(RegisterResp {
        api_key,
        node_id,
        gitea_url: None,
        gitea_user: None,
        gitea_token: None,
        gitea_password: None,
    }))
}

/// Middleware: verify Bearer token against `registered_nodes.api_key_hash`
/// or an externally provisioned `user_tokens.token_hash`, then inject `AuthNode`.
pub async fn auth_middleware(
    State(state): State<Arc<ServerState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let token = match token {
        Some(t) => t.to_string(),
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    let token_hash = hash_api_key(&token);

    let db = state.db.lock().await;
    let node_id: Option<String> = db
        .query_row(
            "SELECT node_id FROM registered_nodes WHERE api_key_hash = ?1",
            rusqlite::params![token_hash],
            |row| row.get(0),
        )
        .ok();

    if let Some(id) = node_id {
        drop(db);
        req.extensions_mut().insert(AuthNode(id));
        return next.run(req).await;
    }

    let user_node: Option<String> = db
        .query_row(
            "SELECT rn.node_id FROM user_tokens ut \
             JOIN registered_nodes rn ON rn.user_id = ut.user_id \
             WHERE ut.token_hash = ?1 \
             AND (ut.expires_at IS NULL OR ut.expires_at > ?2) \
             LIMIT 1",
            rusqlite::params![token_hash, chrono::Utc::now().to_rfc3339()],
            |row| row.get(0),
        )
        .ok();

    if let Some(node_id) = user_node {
        db.execute(
            "UPDATE user_tokens SET last_used_at = ?1 WHERE token_hash = ?2",
            rusqlite::params![chrono::Utc::now().to_rfc3339(), token_hash],
        )
        .ok();
        drop(db);
        req.extensions_mut().insert(AuthNode(node_id));
        return next.run(req).await;
    }

    drop(db);
    StatusCode::UNAUTHORIZED.into_response()
}

/// Helper to extract node_id from Bearer token in headers.
pub async fn extract_node_id(state: &ServerState, headers: &HeaderMap) -> Option<String> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))?;
    let token_hash = hash_api_key(token);
    let db = state.db.lock().await;

    if let Ok(node_id) = db.query_row(
        "SELECT node_id FROM registered_nodes WHERE api_key_hash = ?1",
        rusqlite::params![token_hash],
        |row| row.get::<_, String>(0),
    ) {
        return Some(node_id);
    }

    db.query_row(
        "SELECT rn.node_id FROM user_tokens ut \
         JOIN registered_nodes rn ON rn.user_id = ut.user_id \
         WHERE ut.token_hash = ?1 \
         AND (ut.expires_at IS NULL OR ut.expires_at > ?2) \
         LIMIT 1",
        rusqlite::params![token_hash, chrono::Utc::now().to_rfc3339()],
        |row| row.get(0),
    )
    .ok()
}

/// Resolve a user_id from an externally provisioned Bearer token.
async fn resolve_user_from_bearer(state: &ServerState, headers: &HeaderMap) -> Option<String> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))?;

    let token_hash = hash_api_key(token);
    let db = state.db.lock().await;
    db.query_row(
        "SELECT user_id FROM user_tokens WHERE token_hash = ?1 \
         AND (expires_at IS NULL OR expires_at > ?2)",
        rusqlite::params![token_hash, chrono::Utc::now().to_rfc3339()],
        |row| row.get(0),
    )
    .ok()
}
