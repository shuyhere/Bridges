use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, post};
use axum::{middleware, Extension, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::auth::{auth_middleware, AuthNode};
use super::ServerState;

#[derive(Deserialize)]
pub struct AddContactReq {
    #[serde(rename = "nodeId")]
    pub node_id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

#[derive(Serialize)]
pub struct ContactResp {
    #[serde(rename = "nodeId")]
    pub node_id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "registeredName")]
    pub registered_name: Option<String>,
    #[serde(rename = "addedAt")]
    pub added_at: String,
}

#[derive(Serialize)]
pub struct MessageResp {
    pub ok: bool,
    pub message: String,
}

pub fn routes(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/v1/contacts", post(add_contact).get(list_contacts))
        .route("/v1/contacts/:node_id", delete(remove_contact))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state)
}

async fn add_contact(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
    Json(req): Json<AddContactReq>,
) -> Result<Json<MessageResp>, StatusCode> {
    let db = state.db.lock().await;
    let now = chrono::Utc::now().to_rfc3339();

    // Resolve user_id from the node
    let user_id: String = db
        .query_row(
            "SELECT user_id FROM registered_nodes WHERE node_id = ?1",
            rusqlite::params![auth.0],
            |row| row.get(0),
        )
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Verify the contact node exists
    let exists: bool = db
        .query_row(
            "SELECT 1 FROM registered_nodes WHERE node_id = ?1",
            rusqlite::params![req.node_id],
            |_| Ok(()),
        )
        .is_ok();
    if !exists {
        return Ok(Json(MessageResp {
            ok: false,
            message: format!("Node {} not found", req.node_id),
        }));
    }

    db.execute(
        "INSERT OR REPLACE INTO user_contacts (user_id, contact_node_id, display_name, added_at) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![user_id, req.node_id, req.display_name, now],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(MessageResp {
        ok: true,
        message: format!("Added {} to contacts", req.node_id),
    }))
}

async fn list_contacts(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
) -> Result<Json<Vec<ContactResp>>, StatusCode> {
    let db = state.db.lock().await;

    let user_id: String = db
        .query_row(
            "SELECT user_id FROM registered_nodes WHERE node_id = ?1",
            rusqlite::params![auth.0],
            |row| row.get(0),
        )
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let mut stmt = db
        .prepare(
            "SELECT c.contact_node_id, c.display_name, r.display_name, c.added_at \
             FROM user_contacts c \
             LEFT JOIN registered_nodes r ON r.node_id = c.contact_node_id \
             WHERE c.user_id = ?1 \
             ORDER BY c.added_at DESC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let contacts: Vec<ContactResp> = stmt
        .query_map(rusqlite::params![user_id], |row| {
            Ok(ContactResp {
                node_id: row.get(0)?,
                display_name: row.get(1)?,
                registered_name: row.get(2)?,
                added_at: row.get(3)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(contacts))
}

async fn remove_contact(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
    Path(contact_node_id): Path<String>,
) -> Result<Json<MessageResp>, StatusCode> {
    let db = state.db.lock().await;

    let user_id: String = db
        .query_row(
            "SELECT user_id FROM registered_nodes WHERE node_id = ?1",
            rusqlite::params![auth.0],
            |row| row.get(0),
        )
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let affected = db
        .execute(
            "DELETE FROM user_contacts WHERE user_id = ?1 AND contact_node_id = ?2",
            rusqlite::params![user_id, contact_node_id],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if affected == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(MessageResp {
        ok: true,
        message: "Contact removed".to_string(),
    }))
}
