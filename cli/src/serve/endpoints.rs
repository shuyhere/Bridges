use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{middleware, Extension, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::auth::{auth_middleware, AuthNode};
use super::ServerState;

#[derive(Serialize, Deserialize)]
pub struct EndpointHint {
    pub addr: String,
    #[serde(rename = "hintType")]
    pub hint_type: String,
}

pub fn routes(state: Arc<ServerState>) -> Router {
    // All endpoint lookups require authentication
    Router::new()
        .route("/v1/endpoints/:node_id", get(get_endpoints))
        .route("/v1/endpoints", put(update_endpoints))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state)
}

/// Get a node's endpoint hints. Requires auth — caller must share
/// at least one project with the target node.
async fn get_endpoints(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
    Path(node_id): Path<String>,
) -> Result<Json<Vec<EndpointHint>>, StatusCode> {
    let db = state
        .open_connection()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Verify caller shares at least one project with the target node
    let shares_project: bool = db
        .query_row(
            "SELECT 1 FROM server_members m1 \
             JOIN server_members m2 ON m1.project_id = m2.project_id \
             WHERE m1.node_id = ?1 AND m2.node_id = ?2 LIMIT 1",
            rusqlite::params![auth.0, node_id],
            |_| Ok(()),
        )
        .is_ok();
    if !shares_project {
        return Err(StatusCode::FORBIDDEN);
    }
    let hints_json: String = db
        .query_row(
            "SELECT endpoint_hints FROM registered_nodes WHERE node_id = ?1",
            rusqlite::params![node_id],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "[]".to_string());
    let hints: Vec<EndpointHint> = serde_json::from_str(&hints_json).unwrap_or_default();
    Ok(Json(hints))
}

async fn update_endpoints(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
    Json(hints): Json<Vec<EndpointHint>>,
) -> Result<StatusCode, StatusCode> {
    let json = serde_json::to_string(&hints).map_err(|_| StatusCode::BAD_REQUEST)?;
    let db = state
        .open_connection()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.execute(
        "UPDATE registered_nodes SET endpoint_hints = ?1 WHERE node_id = ?2",
        rusqlite::params![json, auth.0],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}
