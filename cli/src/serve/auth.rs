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

/// Extension: authenticated node (from a node API key).
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
    Json(req): Json<RegisterReq>,
) -> Result<Json<RegisterResp>, StatusCode> {
    let api_key = generate_api_key();
    let key_hash = hash_api_key(&api_key);
    let now = chrono::Utc::now().to_rfc3339();

    let db = state.db.lock().await;
    db.execute(
        "INSERT OR REPLACE INTO registered_nodes \
         (node_id, ed25519_pubkey, x25519_pubkey, display_name, owner_name, api_key_hash, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            req.node_id,
            req.ed25519_pubkey,
            req.x25519_pubkey,
            req.display_name,
            req.owner_name,
            key_hash,
            now,
        ],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(RegisterResp {
        api_key,
        node_id: req.node_id,
    }))
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

    Ok(Json(RegisterResp { api_key, node_id }))
}

/// Middleware: verify Bearer token against `registered_nodes.api_key_hash` and inject `AuthNode`.
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
        Some(t) => t,
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    let token_hash = hash_api_key(token);
    let db = state.db.lock().await;
    let node_id: Option<String> = db
        .query_row(
            "SELECT node_id FROM registered_nodes WHERE api_key_hash = ?1",
            rusqlite::params![token_hash],
            |row| row.get(0),
        )
        .ok();
    drop(db);

    let Some(node_id) = node_id else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    req.extensions_mut().insert(AuthNode(node_id));
    next.run(req).await
}

/// Helper to extract node_id from Bearer token in headers.
pub async fn extract_node_id(state: &ServerState, headers: &HeaderMap) -> Option<String> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))?;
    let token_hash = hash_api_key(token);
    let db = state.db.lock().await;

    db.query_row(
        "SELECT node_id FROM registered_nodes WHERE api_key_hash = ?1",
        rusqlite::params![token_hash],
        |row| row.get(0),
    )
    .ok()
}
