use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine as _;
use ed25519_dalek::VerifyingKey;
use rand::Rng;
use rusqlite::OptionalExtension;
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

#[derive(Clone, Deserialize)]
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

#[derive(Debug, Serialize)]
pub struct RegisterResp {
    #[serde(rename = "apiKey")]
    pub api_key: String,
    #[serde(rename = "nodeId")]
    pub node_id: String,
}

struct ValidatedRegisterReq {
    node_id: String,
    ed25519_pubkey: String,
    x25519_pubkey: String,
    display_name: Option<String>,
    owner_name: Option<String>,
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_ed25519_pubkey(value: &str) -> Result<([u8; 32], VerifyingKey), StatusCode> {
    let decoded = bs58::decode(value)
        .into_vec()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let bytes: [u8; 32] = decoded.try_into().map_err(|_| StatusCode::BAD_REQUEST)?;
    let verifying = VerifyingKey::from_bytes(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok((bytes, verifying))
}

fn parse_x25519_pubkey(value: &str) -> Result<[u8; 32], StatusCode> {
    let decoded = hex::decode(value).map_err(|_| StatusCode::BAD_REQUEST)?;
    decoded.try_into().map_err(|_| StatusCode::BAD_REQUEST)
}

fn validate_register_req(req: &RegisterReq) -> Result<ValidatedRegisterReq, StatusCode> {
    let (ed_pub_bytes, verifying_key) = parse_ed25519_pubkey(&req.ed25519_pubkey)?;
    let x25519_pubkey = parse_x25519_pubkey(&req.x25519_pubkey)?;

    let derived_node_id = crate::identity::derive_node_id(&verifying_key);
    if req.node_id != derived_node_id {
        return Err(StatusCode::BAD_REQUEST);
    }

    let expected_x25519 = crate::crypto::ed25519_to_x25519_public(&ed_pub_bytes)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if x25519_pubkey != expected_x25519 {
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(ValidatedRegisterReq {
        node_id: derived_node_id,
        ed25519_pubkey: bs58::encode(ed_pub_bytes).into_string(),
        x25519_pubkey: hex::encode(expected_x25519),
        display_name: normalize_optional_text(req.display_name.as_deref()),
        owner_name: normalize_optional_text(req.owner_name.as_deref()),
    })
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
    let validated = validate_register_req(&req)?;
    let api_key = generate_api_key();
    let key_hash = hash_api_key(&api_key);
    let now = chrono::Utc::now().to_rfc3339();

    let db = state
        .open_connection()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let existing: Option<(String, String)> = db
        .query_row(
            "SELECT ed25519_pubkey, x25519_pubkey FROM registered_nodes WHERE node_id = ?1",
            rusqlite::params![validated.node_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some((existing_ed25519, existing_x25519)) = existing {
        if existing_ed25519 != validated.ed25519_pubkey
            || existing_x25519 != validated.x25519_pubkey
        {
            return Err(StatusCode::CONFLICT);
        }

        db.execute(
            "UPDATE registered_nodes SET display_name = ?1, owner_name = ?2, api_key_hash = ?3 WHERE node_id = ?4",
            rusqlite::params![
                validated.display_name,
                validated.owner_name,
                key_hash,
                validated.node_id,
            ],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    } else {
        db.execute(
            "INSERT INTO registered_nodes \
             (node_id, ed25519_pubkey, x25519_pubkey, display_name, owner_name, api_key_hash, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                validated.node_id,
                validated.ed25519_pubkey,
                validated.x25519_pubkey,
                validated.display_name,
                validated.owner_name,
                key_hash,
                now,
            ],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    Ok(Json(RegisterResp {
        api_key,
        node_id: validated.node_id,
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

    let db = state
        .open_connection()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
    let db = match state.open_connection() {
        Ok(db) => db,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let node_id: Option<String> = db
        .query_row(
            "SELECT node_id FROM registered_nodes WHERE api_key_hash = ?1",
            rusqlite::params![token_hash],
            |row| row.get(0),
        )
        .ok();

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
    let db = state.open_connection().ok()?;

    db.query_row(
        "SELECT node_id FROM registered_nodes WHERE api_key_hash = ?1",
        rusqlite::params![token_hash],
        |row| row.get(0),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    fn test_state() -> Arc<ServerState> {
        super::super::make_test_state()
    }

    fn register_req_from_signing(signing: &SigningKey) -> RegisterReq {
        let verifying = signing.verifying_key();
        let node_id = crate::identity::derive_node_id(&verifying);
        let x25519 = crate::crypto::ed25519_to_x25519_public(verifying.as_bytes()).unwrap();
        RegisterReq {
            node_id,
            ed25519_pubkey: bs58::encode(verifying.as_bytes()).into_string(),
            x25519_pubkey: hex::encode(x25519),
            display_name: Some("node".to_string()),
            owner_name: Some("owner".to_string()),
        }
    }

    #[tokio::test]
    async fn register_rejects_mismatched_node_id() {
        let state = test_state();
        let signing = SigningKey::generate(&mut OsRng);
        let mut req = register_req_from_signing(&signing);
        req.node_id = "kd_wrong".to_string();

        let result = register(State(state), Json(req)).await;
        assert_eq!(result.unwrap_err(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn register_rejects_mismatched_x25519_pubkey() {
        let state = test_state();
        let signing = SigningKey::generate(&mut OsRng);
        let mut req = register_req_from_signing(&signing);
        req.x25519_pubkey = hex::encode([7u8; 32]);

        let result = register(State(state), Json(req)).await;
        assert_eq!(result.unwrap_err(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn register_rejects_overwrite_of_existing_node_with_different_keys() {
        let state = test_state();
        let signing = SigningKey::generate(&mut OsRng);
        let req = register_req_from_signing(&signing);

        let other = SigningKey::generate(&mut OsRng);
        let other_verifying = other.verifying_key();
        let other_x25519 =
            crate::crypto::ed25519_to_x25519_public(other_verifying.as_bytes()).unwrap();

        let db = state.open_connection().unwrap();
        db.execute(
            "INSERT INTO registered_nodes (node_id, ed25519_pubkey, x25519_pubkey, display_name, owner_name, api_key_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                req.node_id,
                bs58::encode(other_verifying.as_bytes()).into_string(),
                hex::encode(other_x25519),
                "other",
                Option::<String>::None,
                "hash",
                chrono::Utc::now().to_rfc3339(),
            ],
        )
        .unwrap();

        let result = register(State(state), Json(req)).await;
        assert_eq!(result.unwrap_err(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn register_allows_reregister_with_same_keys_and_rotates_api_key() {
        let state = test_state();
        let signing = SigningKey::generate(&mut OsRng);
        let req = register_req_from_signing(&signing);

        let first = register(State(state.clone()), Json(req.clone()))
            .await
            .unwrap()
            .0;
        let second = register(State(state.clone()), Json(req)).await.unwrap().0;

        assert_eq!(first.node_id, second.node_id);
        assert_ne!(first.api_key, second.api_key);
    }

    #[tokio::test]
    async fn refresh_invalidates_old_api_key() {
        let state = test_state();
        let signing = SigningKey::generate(&mut OsRng);
        let req = register_req_from_signing(&signing);
        let registered = register(State(state.clone()), Json(req)).await.unwrap().0;

        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", registered.api_key).parse().unwrap(),
        );
        let refreshed = refresh(State(state.clone()), headers.clone())
            .await
            .unwrap()
            .0;

        let old = extract_node_id(&state, &headers).await;
        assert!(old.is_none());

        let mut new_headers = HeaderMap::new();
        new_headers.insert(
            "authorization",
            format!("Bearer {}", refreshed.api_key).parse().unwrap(),
        );
        let node_id = extract_node_id(&state, &new_headers).await;
        assert_eq!(node_id.as_deref(), Some(refreshed.node_id.as_str()));
    }
}
