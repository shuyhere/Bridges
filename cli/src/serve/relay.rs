use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{middleware, Extension, Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::auth::{auth_middleware, extract_node_id, AuthNode};
use super::ServerState;

/// Relay request: opaque message blob only.
/// For mailbox/direct relay, the server routes the blob without understanding its body.
#[derive(Deserialize)]
pub struct RelayReq {
    /// Target node to deliver the blob to.
    #[serde(rename = "targetNodeId")]
    pub target_node_id: String,
    /// Opaque message blob. For mailbox relay this is an encrypted envelope string.
    pub blob: String,
    /// Optional project ID for authorization-aware decrypt/key lookup on clients.
    #[serde(rename = "projectId")]
    pub project_id: Option<String>,
}

#[derive(Serialize)]
pub struct RelayResp {
    pub delivered: bool,
    pub message: String,
}

/// Broadcast request: sends an opaque encrypted blob to all project members.
/// Each member gets their own copy (sender must encrypt per-peer).
#[derive(Deserialize)]
pub struct BroadcastReq {
    #[serde(rename = "projectId")]
    pub project_id: String,
    /// Map of node_id -> base64-encoded encrypted blob (per-peer encryption).
    pub blobs: std::collections::HashMap<String, String>,
}

#[derive(Serialize)]
pub struct BroadcastResp {
    pub sent_to: Vec<String>,
}

/// Mailbox entry: stores opaque message blobs for later pickup.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MailboxEntry {
    from: String,
    blob: String,
    #[serde(rename = "projectId", skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    timestamp: String,
}

/// In-memory mailbox: encrypted messages waiting for each node.
static MAILBOX: std::sync::OnceLock<
    tokio::sync::Mutex<std::collections::HashMap<String, Vec<MailboxEntry>>>,
> = std::sync::OnceLock::new();

fn mailbox() -> &'static tokio::sync::Mutex<std::collections::HashMap<String, Vec<MailboxEntry>>> {
    MAILBOX.get_or_init(|| tokio::sync::Mutex::new(std::collections::HashMap::new()))
}

#[derive(Debug, Serialize, Deserialize)]
struct DerpFrame {
    src: Option<String>,
    dst: Option<String>,
    #[serde(with = "base64_serde")]
    data: Vec<u8>,
}

mod base64_serde {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(data: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(data))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(&s)
            .map_err(serde::de::Error::custom)
    }
}

static DERP_CLIENTS: std::sync::OnceLock<
    tokio::sync::Mutex<std::collections::HashMap<String, mpsc::UnboundedSender<Message>>>,
> = std::sync::OnceLock::new();

fn derp_clients(
) -> &'static tokio::sync::Mutex<std::collections::HashMap<String, mpsc::UnboundedSender<Message>>>
{
    DERP_CLIENTS.get_or_init(|| tokio::sync::Mutex::new(std::collections::HashMap::new()))
}

pub fn routes(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/v1/relay", post(relay_message))
        .route("/v1/broadcast", post(broadcast_message))
        .route("/v1/mailbox", post(fetch_mailbox))
        .route("/ws/derp", get(derp_ws))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state)
}

/// Max messages per node in the mailbox (prevent memory DoS).
const MAX_MAILBOX_PER_NODE: usize = 1000;
/// Max blob size (64 KB).
const MAX_BLOB_SIZE: usize = 65536;

/// Relay a single opaque blob to a target node.
/// End-to-end confidentiality depends on the sender encrypting the blob body.
async fn relay_message(
    Extension(auth): Extension<AuthNode>,
    Json(req): Json<RelayReq>,
) -> Result<Json<RelayResp>, StatusCode> {
    // Reject oversized blobs
    if req.blob.len() > MAX_BLOB_SIZE {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let entry = MailboxEntry {
        from: auth.0,
        blob: req.blob,
        project_id: req.project_id,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    let mut mb = mailbox().lock().await;
    let queue = mb.entry(req.target_node_id.clone()).or_default();
    // Enforce per-node mailbox limit
    if queue.len() >= MAX_MAILBOX_PER_NODE {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    queue.push(entry);

    Ok(Json(RelayResp {
        delivered: true,
        message: format!("queued for {}", req.target_node_id),
    }))
}

/// Broadcast per-peer message blobs to all specified project members.
/// The sender is responsible for encrypting per-peer blob bodies.
async fn broadcast_message(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthNode>,
    Json(req): Json<BroadcastReq>,
) -> Result<Json<BroadcastResp>, StatusCode> {
    // Verify sender is a member of the project
    let is_member: bool = {
        let db = state.db.lock().await;
        let mut stmt = db
            .prepare("SELECT 1 FROM server_members WHERE project_id = ?1 AND node_id = ?2")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.exists(rusqlite::params![req.project_id, auth.0])
            .unwrap_or(false)
    };
    if !is_member {
        return Err(StatusCode::FORBIDDEN);
    }

    // Validate blob sizes
    for blob in req.blobs.values() {
        if blob.len() > MAX_BLOB_SIZE {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
    }

    let mut mb = mailbox().lock().await;
    let mut sent_to = Vec::new();
    for (node_id, blob) in &req.blobs {
        if *node_id == auth.0 {
            continue;
        }
        let queue = mb.entry(node_id.clone()).or_default();
        if queue.len() >= MAX_MAILBOX_PER_NODE {
            continue;
        } // skip full mailboxes
        let entry = MailboxEntry {
            from: auth.0.clone(),
            blob: blob.clone(),
            project_id: Some(req.project_id.clone()),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        queue.push(entry);
        sent_to.push(node_id.clone());
    }

    Ok(Json(BroadcastResp { sent_to }))
}

/// Fetch and drain pending encrypted messages for this node.
async fn fetch_mailbox(Extension(auth): Extension<AuthNode>) -> Json<Vec<MailboxEntry>> {
    let mut mb = mailbox().lock().await;
    let messages = mb.remove(&auth.0).unwrap_or_default();
    Json(messages)
}

async fn derp_ws(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, StatusCode> {
    let node_id = extract_node_id(&state, &headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    Ok(ws.on_upgrade(move |socket| handle_derp_socket(node_id, socket)))
}

async fn handle_derp_socket(node_id: String, socket: WebSocket) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    {
        let mut clients = derp_clients().lock().await;
        clients.insert(node_id.clone(), tx.clone());
    }

    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = ws_receiver.next().await {
        match msg {
            Message::Text(text) => {
                let frame: DerpFrame = match serde_json::from_str(&text) {
                    Ok(frame) => frame,
                    Err(err) => {
                        eprintln!("DERP parse error from {}: {}", node_id, err);
                        continue;
                    }
                };
                let Some(dst_node_id) = frame.dst else {
                    continue;
                };
                let outbound = DerpFrame {
                    src: Some(node_id.clone()),
                    dst: None,
                    data: frame.data,
                };
                let json = match serde_json::to_string(&outbound) {
                    Ok(json) => json,
                    Err(err) => {
                        eprintln!("DERP serialize error for {}: {}", dst_node_id, err);
                        continue;
                    }
                };

                let peer_tx = {
                    let clients = derp_clients().lock().await;
                    clients.get(&dst_node_id).cloned()
                };
                if let Some(peer_tx) = peer_tx {
                    let _ = peer_tx.send(Message::Text(json));
                }
            }
            Message::Ping(payload) => {
                let _ = tx.send(Message::Pong(payload));
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    {
        let mut clients = derp_clients().lock().await;
        clients.remove(&node_id);
    }
    send_task.abort();
}
