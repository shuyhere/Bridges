use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use super::auth::AuthNode;
use super::ServerState;

#[derive(Deserialize)]
pub struct CreateInviteReq {
    #[serde(rename = "maxUses")]
    pub max_uses: Option<i64>,
}

#[derive(Serialize)]
pub struct InviteResp {
    #[serde(rename = "inviteId")]
    pub invite_id: String,
    #[serde(rename = "inviteToken")]
    pub invite_token: String,
    #[serde(rename = "projectId")]
    pub project_id: String,
}

#[derive(Deserialize)]
pub struct JoinReq {
    #[serde(rename = "inviteToken")]
    pub invite_token: String,
    #[serde(rename = "agentRole")]
    pub agent_role: Option<String>,
}

#[derive(Serialize)]
pub struct JoinResp {
    pub joined: bool,
    #[serde(rename = "projectId")]
    pub project_id: String,
}

#[derive(Serialize)]
pub struct InviteListItem {
    #[serde(rename = "inviteId")]
    pub invite_id: String,
    #[serde(rename = "maxUses")]
    pub max_uses: Option<i64>,
    #[serde(rename = "useCount")]
    pub use_count: i64,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

fn hash_token(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    hex::encode(h.finalize())
}

/// Routes are registered by projects.rs to avoid axum merge conflicts.
/// These handlers are pub so projects.rs can reference them.
pub async fn create_invite_handler(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
    Path(project_id): Path<String>,
    Json(req): Json<CreateInviteReq>,
) -> Result<Json<InviteResp>, StatusCode> {
    let invite_id = format!("inv_{}", uuid::Uuid::new_v4());
    let token = format!("bridges_inv_{}", uuid::Uuid::new_v4());
    let token_hash = hash_token(&token);
    let now = chrono::Utc::now().to_rfc3339();

    let db = state.db.lock().await;
    let is_member: bool = db
        .query_row(
            "SELECT 1 FROM server_members WHERE project_id = ?1 AND node_id = ?2",
            rusqlite::params![project_id, auth.0],
            |_| Ok(()),
        )
        .is_ok();
    if !is_member {
        return Err(StatusCode::FORBIDDEN);
    }
    db.execute(
        "INSERT INTO server_invites (invite_id, project_id, token_hash, created_by, max_uses, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![invite_id, project_id, token_hash, auth.0, req.max_uses, now],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(InviteResp {
        invite_id,
        invite_token: token,
        project_id,
    }))
}

pub async fn join_project_handler(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
    Path(project_id): Path<String>,
    Json(req): Json<JoinReq>,
) -> Result<Json<JoinResp>, StatusCode> {
    let token_hash = hash_token(&req.invite_token);
    let now = chrono::Utc::now().to_rfc3339();
    let joining_node = auth.0;
    let role = req.agent_role.unwrap_or_else(|| "member".to_string());

    let db = state.db.lock().await;

    let (invite_id, max_uses, use_count): (String, Option<i64>, i64) = db
        .query_row(
            "SELECT invite_id, max_uses, use_count FROM server_invites \
             WHERE project_id = ?1 AND token_hash = ?2",
            rusqlite::params![project_id, token_hash],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    if let Some(max) = max_uses {
        if use_count >= max {
            return Err(StatusCode::GONE);
        }
    }

    db.execute(
        "INSERT OR IGNORE INTO server_members (project_id, node_id, agent_role, joined_at) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![project_id, joining_node, role, now],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    db.execute(
        "UPDATE server_invites SET use_count = use_count + 1 WHERE invite_id = ?1",
        rusqlite::params![invite_id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(JoinResp {
        joined: true,
        project_id,
    }))
}

pub async fn list_invites_handler(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<InviteListItem>>, StatusCode> {
    let db = state.db.lock().await;
    let is_member: bool = db
        .query_row(
            "SELECT 1 FROM server_members WHERE project_id = ?1 AND node_id = ?2",
            rusqlite::params![project_id, auth.0],
            |_| Ok(()),
        )
        .is_ok();
    if !is_member {
        return Err(StatusCode::FORBIDDEN);
    }
    let mut stmt = db
        .prepare(
            "SELECT invite_id, max_uses, use_count, created_at \
             FROM server_invites WHERE project_id = ?1",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let invites = stmt
        .query_map(rusqlite::params![project_id], |row| {
            Ok(InviteListItem {
                invite_id: row.get(0)?,
                max_uses: row.get(1)?,
                use_count: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(invites))
}
