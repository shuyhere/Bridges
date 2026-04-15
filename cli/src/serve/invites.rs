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
    #[serde(rename = "nodeId")]
    pub node_id: Option<String>,
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

#[derive(Debug)]
struct RepoAccess {
    owner: String,
    repo: String,
    collaborator: String,
}

fn sanitize_gitea_username(value: &str) -> String {
    value
        .to_lowercase()
        .replace(' ', "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .take(20)
        .collect()
}

async fn add_repo_collaborator(
    gitea_url: &str,
    admin_token: &str,
    owner: &str,
    repo: &str,
    collaborator: &str,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .put(format!(
            "{}/api/v1/repos/{}/{}/collaborators/{}",
            gitea_url, owner, repo, collaborator
        ))
        .header("Authorization", format!("token {}", admin_token))
        .json(&serde_json::json!({ "permission": "write" }))
        .send()
        .await
        .map_err(|e| format!("gitea add collaborator: {}", e))?;

    if resp.status().is_success() {
        return Ok(());
    }

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if status.as_u16() == 422 && text.to_lowercase().contains("already") {
        return Ok(());
    }

    Err(format!("gitea add collaborator HTTP {} — {}", status, text))
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
    // Verify caller is a member of the project
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
    let joining_node = req.node_id.unwrap_or(auth.0);
    let role = req.agent_role.unwrap_or_else(|| "member".to_string());
    let gitea = state.gitea.clone();

    let (invite_id, repo_access): (String, Option<RepoAccess>) = {
        let db = state.db.lock().await;

        // Validate invite.
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

        let repo_access = if gitea.is_some() {
            let row = db
                .query_row(
                    "SELECT p.slug, p.gitea_owner, p.gitea_repo, owner.gitea_user, owner.display_name, joiner.gitea_user, joiner.display_name \
                     FROM server_projects p \
                     LEFT JOIN registered_nodes owner ON owner.node_id = p.created_by \
                     LEFT JOIN registered_nodes joiner ON joiner.node_id = ?2 \
                     WHERE p.project_id = ?1",
                    rusqlite::params![project_id, joining_node],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                        ))
                    },
                )
                .ok();

            row.and_then(
                |(
                    slug,
                    project_owner,
                    project_repo,
                    owner_gitea_user,
                    owner_display,
                    joiner_gitea_user,
                    joiner_display,
                )| {
                    let owner = project_owner
                        .or(owner_gitea_user)
                        .or_else(|| owner_display.map(|s| sanitize_gitea_username(&s)))
                        .filter(|s| !s.is_empty())?;
                    let repo = project_repo.or(Some(slug)).filter(|s| !s.is_empty())?;
                    let collaborator = joiner_gitea_user
                        .or_else(|| joiner_display.map(|s| sanitize_gitea_username(&s)))
                        .filter(|s| !s.is_empty())?;
                    Some(RepoAccess {
                        owner,
                        repo,
                        collaborator,
                    })
                },
            )
        } else {
            None
        };

        (invite_id, repo_access)
    };

    if let (Some(gitea), Some(repo)) = (gitea.as_ref(), repo_access.as_ref()) {
        add_repo_collaborator(
            &gitea.gitea_url,
            &gitea.admin_token,
            &repo.owner,
            &repo.repo,
            &repo.collaborator,
        )
        .await
        .map_err(|err| {
            eprintln!("Join failed while provisioning Gitea access: {}", err);
            StatusCode::BAD_GATEWAY
        })?;
    }

    let db = state.db.lock().await;

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
    // Verify caller is a member of the project
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
