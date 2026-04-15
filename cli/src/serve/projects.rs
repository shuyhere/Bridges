use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{middleware, Extension, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::auth::{auth_middleware, AuthNode};
use super::ServerState;

#[derive(Deserialize)]
pub struct CreateProjectReq {
    pub slug: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "giteaOwner")]
    pub gitea_owner: Option<String>,
    #[serde(rename = "giteaRepo")]
    pub gitea_repo: Option<String>,
}

#[derive(Serialize)]
pub struct ProjectResp {
    #[serde(rename = "projectId")]
    pub project_id: String,
    pub slug: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "createdBy")]
    pub created_by: String,
    #[serde(rename = "giteaOwner")]
    pub gitea_owner: Option<String>,
    #[serde(rename = "giteaRepo")]
    pub gitea_repo: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Serialize)]
pub struct MemberResp {
    #[serde(rename = "nodeId")]
    pub node_id: String,
    #[serde(rename = "agentRole")]
    pub agent_role: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "ed25519Pubkey")]
    pub ed25519_pubkey: Option<String>,
    #[serde(rename = "joinedAt")]
    pub joined_at: String,
}

pub fn routes(state: Arc<ServerState>) -> Router {
    // All project + invite + join routes in one router to avoid axum merge conflicts
    Router::new()
        .route("/v1/projects", post(create_project))
        .route("/v1/projects", get(list_projects))
        .route("/v1/projects/:id", get(get_project))
        .route("/v1/projects/:id/members", get(list_members))
        .route(
            "/v1/projects/:id/invites",
            axum::routing::post(super::invites::create_invite_handler)
                .get(super::invites::list_invites_handler),
        )
        .route(
            "/v1/projects/:id/join",
            axum::routing::post(super::invites::join_project_handler),
        )
        // Sync goes through /v1/relay (zero-knowledge encrypted blobs)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state)
}

async fn create_project(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
    Json(req): Json<CreateProjectReq>,
) -> Result<Json<ProjectResp>, StatusCode> {
    let project_id = format!("proj_{}", uuid::Uuid::new_v4());
    let now = chrono::Utc::now().to_rfc3339();
    let gitea_repo = req.gitea_repo.clone().or_else(|| Some(req.slug.clone()));

    let db = state.db.lock().await;
    db.execute(
        "INSERT INTO server_projects (project_id, slug, display_name, description, created_by, gitea_owner, gitea_repo, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![project_id, req.slug, req.display_name, req.description, auth.0, req.gitea_owner, gitea_repo, now],
    )
    .map_err(|_| StatusCode::CONFLICT)?;

    // Creator auto-joins as owner.
    db.execute(
        "INSERT INTO server_members (project_id, node_id, agent_role, joined_at) \
         VALUES (?1, ?2, 'owner', ?3)",
        rusqlite::params![project_id, auth.0, now],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ProjectResp {
        project_id,
        slug: req.slug,
        display_name: req.display_name,
        description: req.description,
        created_by: auth.0,
        gitea_owner: req.gitea_owner,
        gitea_repo,
        created_at: now,
    }))
}

async fn list_projects(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
) -> Result<Json<Vec<ProjectResp>>, StatusCode> {
    let db = state.db.lock().await;
    let mut stmt = db
        .prepare(
            "SELECT p.project_id, p.slug, p.display_name, p.description, p.created_by, p.gitea_owner, p.gitea_repo, p.created_at \
             FROM server_projects p \
             JOIN server_members m ON p.project_id = m.project_id \
             WHERE m.node_id = ?1",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let projects = stmt
        .query_map(rusqlite::params![auth.0], |row| {
            Ok(ProjectResp {
                project_id: row.get(0)?,
                slug: row.get(1)?,
                display_name: row.get(2)?,
                description: row.get(3)?,
                created_by: row.get(4)?,
                gitea_owner: row.get(5)?,
                gitea_repo: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(projects))
}

async fn get_project(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
    Path(id): Path<String>,
) -> Result<Json<ProjectResp>, StatusCode> {
    let db = state.db.lock().await;
    // Verify caller is a member of the project
    let is_member: bool = db
        .query_row(
            "SELECT 1 FROM server_members WHERE project_id = ?1 AND node_id = ?2",
            rusqlite::params![id, auth.0],
            |_| Ok(()),
        )
        .is_ok();
    if !is_member {
        return Err(StatusCode::FORBIDDEN);
    }
    db.query_row(
        "SELECT project_id, slug, display_name, description, created_by, gitea_owner, gitea_repo, created_at \
         FROM server_projects WHERE project_id = ?1",
        rusqlite::params![id],
        |row| {
            Ok(ProjectResp {
                project_id: row.get(0)?,
                slug: row.get(1)?,
                display_name: row.get(2)?,
                description: row.get(3)?,
                created_by: row.get(4)?,
                gitea_owner: row.get(5)?,
                gitea_repo: row.get(6)?,
                created_at: row.get(7)?,
            })
        },
    )
    .map(Json)
    .map_err(|_| StatusCode::NOT_FOUND)
}

async fn list_members(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
    Path(id): Path<String>,
) -> Result<Json<Vec<MemberResp>>, StatusCode> {
    let db = state.db.lock().await;
    // Verify caller is a member of the project
    let is_member: bool = db
        .query_row(
            "SELECT 1 FROM server_members WHERE project_id = ?1 AND node_id = ?2",
            rusqlite::params![id, auth.0],
            |_| Ok(()),
        )
        .is_ok();
    if !is_member {
        return Err(StatusCode::FORBIDDEN);
    }
    let mut stmt = db
        .prepare(
            "SELECT m.node_id, m.agent_role, r.display_name, r.ed25519_pubkey, m.joined_at \
             FROM server_members m \
             LEFT JOIN registered_nodes r ON m.node_id = r.node_id \
             WHERE m.project_id = ?1",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let members = stmt
        .query_map(rusqlite::params![id], |row| {
            Ok(MemberResp {
                node_id: row.get(0)?,
                agent_role: row.get(1)?,
                display_name: row.get(2)?,
                ed25519_pubkey: row.get(3)?,
                joined_at: row.get(4)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Json(members))
}
