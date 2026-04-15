use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use crate::coord_client::{CoordClient, MemberInfo, PeerKeys};
use crate::transport::Transport;

/// Pending response from a peer.
pub struct PendingResponse {
    pub response: Option<String>,
    pub from_node: Option<String>,
    pub created_at: Instant,
    pub project_id: Option<String>,
    pub kind: Option<String>,
    pub prompt: Option<String>,
    pub session_id: Option<String>,
}

/// State shared with axum handlers.
pub struct ApiState {
    pub transport: Arc<Transport>,
    pub coord: Arc<dyn CoordOps>,
    pub node_id: String,
    pub my_x25519_priv: [u8; 32],
    /// Pending responses keyed by request_id.
    pub responses: Arc<Mutex<HashMap<String, PendingResponse>>>,
}

#[async_trait::async_trait]
pub trait CoordOps: Send + Sync {
    async fn get_peer_keys(&self, peer_id: &str) -> Result<PeerKeys, String>;
    async fn get_project_keys(&self, project_id: &str) -> Result<Vec<PeerKeys>, String>;
    async fn relay_message(
        &self,
        target_node_id: &str,
        blob: &str,
        project_id: Option<&str>,
    ) -> Result<(), String>;
    async fn get_project_members(&self, project_id: &str) -> Result<Vec<MemberInfo>, String>;
}

#[async_trait::async_trait]
impl CoordOps for CoordClient {
    async fn get_peer_keys(&self, peer_id: &str) -> Result<PeerKeys, String> {
        CoordClient::get_peer_keys(self, peer_id).await
    }

    async fn get_project_keys(&self, project_id: &str) -> Result<Vec<PeerKeys>, String> {
        CoordClient::get_project_keys(self, project_id).await
    }

    async fn relay_message(
        &self,
        target_node_id: &str,
        blob: &str,
        project_id: Option<&str>,
    ) -> Result<(), String> {
        CoordClient::relay_message(self, target_node_id, blob, project_id).await
    }

    async fn get_project_members(&self, project_id: &str) -> Result<Vec<MemberInfo>, String> {
        CoordClient::get_project_members(self, project_id).await
    }
}

#[derive(Debug, Deserialize)]
pub struct SendRequest {
    pub peer_id: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct SendResponse {
    pub ok: bool,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub state: String,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub node_id: String,
    pub healthy: bool,
}

// ── Structured message requests ──

#[derive(Debug, Deserialize)]
pub struct AskRequest {
    pub node_id: String,
    pub question: String,
    pub project_id: String,
    #[serde(default)]
    pub new_session: bool,
}

#[derive(Debug, Deserialize)]
pub struct BroadcastRequest {
    pub message: String,
    pub project_id: String,
    #[serde(default = "default_message_type")]
    pub message_type: String,
}

fn default_message_type() -> String {
    "broadcast".to_string()
}

#[derive(Debug, Deserialize)]
pub struct DebateRequest {
    pub topic: String,
    pub project_id: String,
    #[serde(default)]
    pub new_session: bool,
}

#[derive(Debug, Deserialize)]
pub struct PublishRequest {
    pub filename: String,
    pub data: String,
    pub project_id: String,
}

#[derive(Debug, Serialize)]
pub struct BroadcastResponse {
    pub ok: bool,
    pub sent_to: Vec<String>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct PollResponse {
    pub ready: bool,
    pub from_node: Option<String>,
    pub response: Option<String>,
}

/// Build the axum router for the local API.
pub fn router(state: Arc<ApiState>) -> Router {
    Router::new()
        .route("/send", post(handle_send))
        .route("/ask", post(handle_ask))
        .route("/broadcast", post(handle_broadcast))
        .route("/debate", post(handle_debate))
        .route("/publish", post(handle_publish))
        .route("/response/:id", get(handle_poll_response))
        .route("/peers", get(handle_peers))
        .route("/status", get(handle_status))
        .with_state(state)
}

/// Start the local API server on 127.0.0.1:<port>.
pub async fn serve(state: Arc<ApiState>, port: u16) -> Result<(), String> {
    let app = router(state);
    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("bind {}: {}", addr, e))?;
    println!("Local API listening on {}", addr);
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve: {}", e))
}

/// Ensure Noise handshake is complete with a peer, then encrypt and send.
async fn encrypt_and_send(
    state: &ApiState,
    peer_id: &str,
    project_id: Option<&str>,
    payload: &serde_json::Value,
) -> Result<(), String> {
    use crate::connmgr::SessionState;
    use crate::noise;

    let plaintext = serde_json::to_vec(payload).map_err(|e| format!("serialize: {}", e))?;

    let resolve_peer_keys = || async {
        if let Some(project_id) = project_id {
            let keys = state.coord.get_project_keys(project_id).await?;
            if let Some(keys) = keys.into_iter().find(|keys| keys.node_id == peer_id) {
                return Ok(keys);
            }
            return Err(format!(
                "peer {} not found in project {} key list",
                peer_id, project_id
            ));
        }
        state.coord.get_peer_keys(peer_id).await
    };

    let relay_encrypted = || async {
        let keys = resolve_peer_keys().await?;
        let decoded =
            hex::decode(&keys.x25519_pub).map_err(|e| format!("bad x25519 pubkey: {}", e))?;
        if decoded.len() != 32 {
            return Err("x25519 pubkey wrong length".to_string());
        }
        let mut x_pub = [0u8; 32];
        x_pub.copy_from_slice(&decoded);
        let blob = crate::crypto::encrypt_mailbox_payload(
            &state.node_id,
            peer_id,
            &state.my_x25519_priv,
            &x_pub,
            &plaintext,
        )?;
        state.coord.relay_message(peer_id, &blob, project_id).await
    };

    let needs_handshake = {
        let conn = state.transport.conn.lock().await;
        match conn.peers.get(peer_id) {
            Some(pc) => match &pc.session {
                SessionState::Established(session) => noise::needs_rekey(session),
                _ => true,
            },
            None => true,
        }
    };

    if needs_handshake {
        {
            let mut conn = state.transport.conn.lock().await;
            if let Some(pc) = conn.peers.get_mut(peer_id) {
                if let SessionState::Established(_) = pc.session {
                    let old = std::mem::replace(&mut pc.session, SessionState::None);
                    if let SessionState::Established(s) = old {
                        pc.previous_session = Some(s);
                    }
                }
            }
        }

        let keys = resolve_peer_keys().await?;
        let mut x_pub = [0u8; 32];
        let decoded =
            hex::decode(&keys.x25519_pub).map_err(|e| format!("bad x25519 pubkey: {}", e))?;
        if decoded.len() != 32 {
            return Err("x25519 pubkey wrong length".to_string());
        }
        x_pub.copy_from_slice(&decoded);
        if let Err(e) = state.transport.handshake(peer_id, &x_pub).await {
            eprintln!(
                "  direct handshake to {} failed ({}), using server relay",
                peer_id, e
            );
            return relay_encrypted().await;
        }
    }

    // Try direct transport first (Noise IK via DERP/UDP)
    match state.transport.send(peer_id, &plaintext).await {
        Ok(_) => Ok(()),
        Err(e) => {
            // Fallback: relay through coordination server mailbox
            eprintln!("  direct send failed ({}), using server relay", e);
            relay_encrypted().await
        }
    }
}

/// Store a pending request and return its ID.
fn resolve_project_dir(project_id: &str) -> Option<String> {
    let conn = crate::db::open_db();
    crate::db::init_db(&conn);
    crate::queries::get_project_path(&conn, project_id)
}

fn new_request_id() -> String {
    format!("req_{}", uuid::Uuid::new_v4())
}

async fn insert_pending(
    state: &ApiState,
    request_id: String,
    project_id: &str,
    kind: &str,
    prompt: &str,
    session_id: Option<String>,
) {
    let mut responses = state.responses.lock().await;
    responses.insert(
        request_id,
        PendingResponse {
            response: None,
            from_node: None,
            created_at: Instant::now(),
            project_id: if project_id.trim().is_empty() {
                None
            } else {
                Some(project_id.to_string())
            },
            kind: Some(kind.to_string()),
            prompt: Some(prompt.to_string()),
            session_id,
        },
    );
    // Clean up old entries (>5 minutes)
    responses.retain(|_, v| v.created_at.elapsed().as_secs() < 300);
}

async fn remove_pending(state: &ApiState, request_id: &str) {
    let mut responses = state.responses.lock().await;
    responses.remove(request_id);
}

fn require_non_empty(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{} is required", field))
    } else {
        Ok(())
    }
}

/// Called by the daemon recv loop when a response message arrives.
pub async fn store_response(
    responses: &Arc<Mutex<HashMap<String, PendingResponse>>>,
    request_id: &str,
    from_node: &str,
    response_text: &str,
) {
    let mut exchange = None;
    let mut map = responses.lock().await;
    if let Some(pending) = map.get_mut(request_id) {
        pending.response = Some(response_text.to_string());
        pending.from_node = Some(from_node.to_string());
        exchange = Some((
            pending.project_id.clone().unwrap_or_default(),
            pending.kind.clone().unwrap_or_else(|| "ask".to_string()),
            pending.prompt.clone().unwrap_or_default(),
            pending.session_id.clone(),
        ));
    }
    drop(map);

    if let Some((project_id, kind, prompt, session_id)) = exchange {
        if let Some(project_dir) = resolve_project_dir(&project_id) {
            let _ = crate::conversation_memory::append_exchange(
                &project_dir,
                from_node,
                session_id.as_deref(),
                &kind,
                &prompt,
                response_text,
            );
        }
    }
}

async fn handle_send(
    State(state): State<Arc<ApiState>>,
    Json(req): Json<SendRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_non_empty(&req.peer_id, "peer_id") {
        return (
            StatusCode::BAD_REQUEST,
            Json(SendResponse {
                ok: false,
                error: Some(e),
                request_id: None,
            }),
        );
    }
    if let Err(e) = require_non_empty(&req.message, "message") {
        return (
            StatusCode::BAD_REQUEST,
            Json(SendResponse {
                ok: false,
                error: Some(e),
                request_id: None,
            }),
        );
    }

    let peer_id = req.peer_id.trim();
    let payload = serde_json::json!({
        "from": state.node_id,
        "messageType": "raw",
        "payload": { "message": req.message },
    });
    match encrypt_and_send(&state, peer_id, None, &payload).await {
        Ok(_) => (
            StatusCode::OK,
            Json(SendResponse {
                ok: true,
                error: None,
                request_id: None,
            }),
        ),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(SendResponse {
                ok: false,
                error: Some(e),
                request_id: None,
            }),
        ),
    }
}

async fn handle_ask(
    State(state): State<Arc<ApiState>>,
    Json(req): Json<AskRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_non_empty(&req.node_id, "node_id") {
        return (
            StatusCode::BAD_REQUEST,
            Json(SendResponse {
                ok: false,
                error: Some(e),
                request_id: None,
            }),
        );
    }
    if let Err(e) = require_non_empty(&req.question, "question") {
        return (
            StatusCode::BAD_REQUEST,
            Json(SendResponse {
                ok: false,
                error: Some(e),
                request_id: None,
            }),
        );
    }

    let node_id = req.node_id.trim();
    let project_id = req.project_id.trim();
    let session_id = if project_id.is_empty() {
        None
    } else {
        resolve_project_dir(project_id).and_then(|project_dir| {
            crate::conversation_memory::resolve_session(
                &project_dir,
                node_id,
                None,
                req.new_session,
            )
            .ok()
        })
    };
    let request_id = new_request_id();
    insert_pending(
        &state,
        request_id.clone(),
        project_id,
        "ask",
        &req.question,
        session_id.clone(),
    )
    .await;
    let payload = serde_json::json!({
        "from": state.node_id,
        "projectId": project_id,
        "messageType": "ask",
        "requestId": request_id,
        "sessionId": session_id,
        "payload": { "question": req.question },
    });
    let project_ref = if project_id.is_empty() {
        None
    } else {
        Some(project_id)
    };
    match encrypt_and_send(&state, node_id, project_ref, &payload).await {
        Ok(_) => (
            StatusCode::OK,
            Json(SendResponse {
                ok: true,
                error: None,
                request_id: Some(request_id),
            }),
        ),
        Err(e) => {
            remove_pending(&state, &request_id).await;
            (
                StatusCode::BAD_GATEWAY,
                Json(SendResponse {
                    ok: false,
                    error: Some(e),
                    request_id: None,
                }),
            )
        }
    }
}

async fn handle_broadcast(
    State(state): State<Arc<ApiState>>,
    Json(req): Json<BroadcastRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_non_empty(&req.project_id, "project_id") {
        return (
            StatusCode::BAD_REQUEST,
            Json(BroadcastResponse {
                ok: false,
                sent_to: vec![],
                error: Some(e),
                request_ids: None,
            }),
        );
    }
    if let Err(e) = require_non_empty(&req.message, "message") {
        return (
            StatusCode::BAD_REQUEST,
            Json(BroadcastResponse {
                ok: false,
                sent_to: vec![],
                error: Some(e),
                request_ids: None,
            }),
        );
    }
    if let Err(e) = require_non_empty(&req.message_type, "message_type") {
        return (
            StatusCode::BAD_REQUEST,
            Json(BroadcastResponse {
                ok: false,
                sent_to: vec![],
                error: Some(e),
                request_ids: None,
            }),
        );
    }

    let project_id = req.project_id.trim();
    let members = match state.coord.get_project_members(project_id).await {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(BroadcastResponse {
                    ok: false,
                    sent_to: vec![],
                    error: Some(e),
                    request_ids: None,
                }),
            )
        }
    };

    let mut sent_to = Vec::new();
    let mut last_err = None;
    for member in &members {
        if member.node_id == state.node_id {
            continue;
        }
        let payload = serde_json::json!({
            "from": state.node_id,
            "projectId": project_id,
            "messageType": req.message_type,
            "payload": { "message": req.message },
        });
        match encrypt_and_send(&state, &member.node_id, Some(project_id), &payload).await {
            Ok(_) => sent_to.push(member.node_id.clone()),
            Err(e) => last_err = Some(e),
        }
    }

    let status = if sent_to.is_empty() && last_err.is_some() {
        StatusCode::BAD_GATEWAY
    } else {
        StatusCode::OK
    };

    (
        status,
        Json(BroadcastResponse {
            ok: last_err.is_none(),
            sent_to,
            error: last_err,
            request_ids: None,
        }),
    )
}

async fn handle_debate(
    State(state): State<Arc<ApiState>>,
    Json(req): Json<DebateRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_non_empty(&req.project_id, "project_id") {
        return (
            StatusCode::BAD_REQUEST,
            Json(BroadcastResponse {
                ok: false,
                sent_to: vec![],
                error: Some(e),
                request_ids: None,
            }),
        );
    }
    if let Err(e) = require_non_empty(&req.topic, "topic") {
        return (
            StatusCode::BAD_REQUEST,
            Json(BroadcastResponse {
                ok: false,
                sent_to: vec![],
                error: Some(e),
                request_ids: None,
            }),
        );
    }

    let project_id = req.project_id.trim();
    let members = match state.coord.get_project_members(project_id).await {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(BroadcastResponse {
                    ok: false,
                    sent_to: vec![],
                    error: Some(e),
                    request_ids: None,
                }),
            )
        }
    };

    let mut sent_to = Vec::new();
    let mut request_ids = Vec::new();
    let mut last_err = None;
    for member in &members {
        if member.node_id == state.node_id {
            continue;
        }
        let session_id = resolve_project_dir(project_id).and_then(|project_dir| {
            crate::conversation_memory::resolve_session(
                &project_dir,
                &member.node_id,
                None,
                req.new_session,
            )
            .ok()
        });
        let request_id = new_request_id();
        insert_pending(
            &state,
            request_id.clone(),
            project_id,
            "debate",
            &req.topic,
            session_id.clone(),
        )
        .await;
        let payload = serde_json::json!({
            "from": state.node_id,
            "projectId": project_id,
            "messageType": "debate",
            "requestId": request_id,
            "sessionId": session_id,
            "payload": { "topic": req.topic },
        });
        match encrypt_and_send(&state, &member.node_id, Some(project_id), &payload).await {
            Ok(_) => {
                sent_to.push(member.node_id.clone());
                request_ids.push(request_id);
            }
            Err(e) => {
                remove_pending(&state, &request_id).await;
                last_err = Some(e);
            }
        }
    }

    let status = if sent_to.is_empty() && last_err.is_some() {
        StatusCode::BAD_GATEWAY
    } else {
        StatusCode::OK
    };

    (
        status,
        Json(BroadcastResponse {
            ok: last_err.is_none(),
            sent_to,
            error: last_err,
            request_ids: Some(request_ids),
        }),
    )
}

async fn handle_publish(
    State(state): State<Arc<ApiState>>,
    Json(req): Json<PublishRequest>,
) -> impl IntoResponse {
    use base64::Engine;

    if let Err(e) = require_non_empty(&req.project_id, "project_id") {
        return (
            StatusCode::BAD_REQUEST,
            Json(BroadcastResponse {
                ok: false,
                sent_to: vec![],
                error: Some(e),
                request_ids: None,
            }),
        );
    }
    if let Err(e) = require_non_empty(&req.filename, "filename") {
        return (
            StatusCode::BAD_REQUEST,
            Json(BroadcastResponse {
                ok: false,
                sent_to: vec![],
                error: Some(e),
                request_ids: None,
            }),
        );
    }
    if let Err(e) = require_non_empty(&req.data, "data") {
        return (
            StatusCode::BAD_REQUEST,
            Json(BroadcastResponse {
                ok: false,
                sent_to: vec![],
                error: Some(e),
                request_ids: None,
            }),
        );
    }
    if let Err(e) = base64::engine::general_purpose::STANDARD.decode(&req.data) {
        return (
            StatusCode::BAD_REQUEST,
            Json(BroadcastResponse {
                ok: false,
                sent_to: vec![],
                error: Some(format!("data must be valid base64: {}", e)),
                request_ids: None,
            }),
        );
    }

    let project_id = req.project_id.trim();
    let members = match state.coord.get_project_members(project_id).await {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(BroadcastResponse {
                    ok: false,
                    sent_to: vec![],
                    error: Some(e),
                    request_ids: None,
                }),
            )
        }
    };

    let mut sent_to = Vec::new();
    let mut last_err = None;
    for member in &members {
        if member.node_id == state.node_id {
            continue;
        }
        let payload = serde_json::json!({
            "from": state.node_id,
            "projectId": project_id,
            "messageType": "publish",
            "payload": { "filename": req.filename, "data": req.data },
        });
        match encrypt_and_send(&state, &member.node_id, Some(project_id), &payload).await {
            Ok(_) => sent_to.push(member.node_id.clone()),
            Err(e) => last_err = Some(e),
        }
    }

    let status = if sent_to.is_empty() && last_err.is_some() {
        StatusCode::BAD_GATEWAY
    } else {
        StatusCode::OK
    };

    (
        status,
        Json(BroadcastResponse {
            ok: last_err.is_none(),
            sent_to,
            error: last_err,
            request_ids: None,
        }),
    )
}

/// Poll for a response by request_id.
async fn handle_poll_response(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Json<PollResponse> {
    let mut map = state.responses.lock().await;
    if let Some(pending) = map.get(&id) {
        if let Some(ref resp) = pending.response {
            let result = PollResponse {
                ready: true,
                from_node: pending.from_node.clone(),
                response: Some(resp.clone()),
            };
            map.remove(&id); // clean up after reading
            return Json(result);
        }
    }
    Json(PollResponse {
        ready: false,
        from_node: None,
        response: None,
    })
}

async fn handle_peers(State(state): State<Arc<ApiState>>) -> Json<Vec<PeerInfo>> {
    let conn = state.transport.conn.lock().await;
    let peers: Vec<PeerInfo> = conn
        .peers
        .iter()
        .map(|(id, pc)| PeerInfo {
            peer_id: id.clone(),
            state: format!("{:?}", pc.state),
        })
        .collect();
    Json(peers)
}

async fn handle_status(State(state): State<Arc<ApiState>>) -> Json<StatusResponse> {
    Json(StatusResponse {
        node_id: state.node_id.clone(),
        healthy: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connmgr::ConnManager;
    use crate::crypto;
    use crate::transport::Transport;
    use axum::body::to_bytes;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use std::sync::Arc;

    #[derive(Clone)]
    struct MockCoord {
        peer_x25519_pub: String,
        relayed: Arc<Mutex<Vec<RelayReq>>>,
        relay_error: Option<String>,
        project_members: Vec<MemberInfo>,
    }

    #[derive(Clone, Debug)]
    struct RelayReq {
        target_node_id: String,
        blob: String,
        project_id: Option<String>,
    }

    #[async_trait::async_trait]
    impl CoordOps for MockCoord {
        async fn get_peer_keys(&self, peer_id: &str) -> Result<PeerKeys, String> {
            Ok(PeerKeys {
                node_id: peer_id.to_string(),
                ed25519_pub: "unused".to_string(),
                x25519_pub: self.peer_x25519_pub.clone(),
            })
        }

        async fn get_project_keys(&self, project_id: &str) -> Result<Vec<PeerKeys>, String> {
            Ok(vec![PeerKeys {
                node_id: if project_id == "proj_test" {
                    "kd_peer".to_string()
                } else {
                    "kd_unknown".to_string()
                },
                ed25519_pub: "unused".to_string(),
                x25519_pub: self.peer_x25519_pub.clone(),
            }])
        }

        async fn relay_message(
            &self,
            target_node_id: &str,
            blob: &str,
            project_id: Option<&str>,
        ) -> Result<(), String> {
            self.relayed.lock().await.push(RelayReq {
                target_node_id: target_node_id.to_string(),
                blob: blob.to_string(),
                project_id: project_id.map(str::to_string),
            });
            if let Some(err) = &self.relay_error {
                return Err(err.clone());
            }
            Ok(())
        }

        async fn get_project_members(&self, _project_id: &str) -> Result<Vec<MemberInfo>, String> {
            Ok(self.project_members.clone())
        }
    }

    fn make_test_state(coord: Arc<dyn CoordOps>) -> ApiState {
        let signing = SigningKey::generate(&mut OsRng);
        let x_priv = crypto::ed25519_to_x25519_private(&signing.to_bytes());
        let transport = Arc::new(Transport::new_for_tests(
            ConnManager::new(None),
            None,
            "kd_sender".to_string(),
            x_priv,
        ));
        ApiState {
            transport,
            coord,
            node_id: "kd_sender".to_string(),
            my_x25519_priv: x_priv,
            responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn response_json(response: impl IntoResponse) -> (StatusCode, serde_json::Value) {
        let response = response.into_response();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&body).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn encrypt_and_send_relays_when_direct_handshake_is_unavailable() {
        let peer_signing = SigningKey::generate(&mut OsRng);
        let peer_x25519 =
            crypto::ed25519_to_x25519_public(peer_signing.verifying_key().as_bytes()).unwrap();
        let relayed = Arc::new(Mutex::new(Vec::new()));
        let coord = Arc::new(MockCoord {
            peer_x25519_pub: hex::encode(peer_x25519),
            relayed: relayed.clone(),
            relay_error: None,
            project_members: Vec::new(),
        });

        let signing = SigningKey::generate(&mut OsRng);
        let x_priv = crypto::ed25519_to_x25519_private(&signing.to_bytes());
        let transport = Arc::new(Transport::new_for_tests(
            ConnManager::new(None),
            None,
            "kd_sender".to_string(),
            x_priv,
        ));
        let state = ApiState {
            transport,
            coord,
            node_id: "kd_sender".to_string(),
            my_x25519_priv: x_priv,
            responses: Arc::new(Mutex::new(HashMap::new())),
        };
        let payload = serde_json::json!({
            "from": "kd_sender",
            "projectId": "proj_test",
            "messageType": "ask",
            "payload": { "question": "What is this project about?" },
        });

        encrypt_and_send(&state, "kd_peer", Some("proj_test"), &payload)
            .await
            .unwrap();

        let relayed = relayed.lock().await;
        assert_eq!(relayed.len(), 1);
        assert_eq!(relayed[0].target_node_id, "kd_peer");
        assert_eq!(relayed[0].project_id.as_deref(), Some("proj_test"));

        let sender_pub =
            crypto::ed25519_to_x25519_public(signing.verifying_key().as_bytes()).unwrap();
        let plaintext = crypto::decrypt_mailbox_payload(
            "kd_peer",
            "kd_sender",
            &crypto::ed25519_to_x25519_private(&peer_signing.to_bytes()),
            &sender_pub,
            &relayed[0].blob,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();
        assert_eq!(parsed["messageType"], "ask");
        assert_eq!(parsed["payload"]["question"], "What is this project about?");
    }

    #[tokio::test]
    async fn handle_send_rejects_empty_peer_id() {
        let peer_signing = SigningKey::generate(&mut OsRng);
        let peer_x25519 =
            crypto::ed25519_to_x25519_public(peer_signing.verifying_key().as_bytes()).unwrap();
        let state = Arc::new(make_test_state(Arc::new(MockCoord {
            peer_x25519_pub: hex::encode(peer_x25519),
            relayed: Arc::new(Mutex::new(Vec::new())),
            relay_error: None,
            project_members: Vec::new(),
        })));

        let (status, json) = response_json(
            handle_send(
                State(state),
                Json(SendRequest {
                    peer_id: "   ".to_string(),
                    message: "hello".to_string(),
                }),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"], "peer_id is required");
    }

    #[tokio::test]
    async fn handle_ask_removes_pending_when_delivery_fails() {
        let peer_signing = SigningKey::generate(&mut OsRng);
        let peer_x25519 =
            crypto::ed25519_to_x25519_public(peer_signing.verifying_key().as_bytes()).unwrap();
        let state = Arc::new(make_test_state(Arc::new(MockCoord {
            peer_x25519_pub: hex::encode(peer_x25519),
            relayed: Arc::new(Mutex::new(Vec::new())),
            relay_error: Some("relay unavailable".to_string()),
            project_members: Vec::new(),
        })));

        let (status, json) = response_json(
            handle_ask(
                State(state.clone()),
                Json(AskRequest {
                    node_id: "kd_peer".to_string(),
                    question: "hello?".to_string(),
                    project_id: "".to_string(),
                    new_session: false,
                }),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(json["ok"], false);
        assert_eq!(state.responses.lock().await.len(), 0);
    }

    #[tokio::test]
    async fn handle_publish_rejects_invalid_base64() {
        let peer_signing = SigningKey::generate(&mut OsRng);
        let peer_x25519 =
            crypto::ed25519_to_x25519_public(peer_signing.verifying_key().as_bytes()).unwrap();
        let state = Arc::new(make_test_state(Arc::new(MockCoord {
            peer_x25519_pub: hex::encode(peer_x25519),
            relayed: Arc::new(Mutex::new(Vec::new())),
            relay_error: None,
            project_members: Vec::new(),
        })));

        let (status, json) = response_json(
            handle_publish(
                State(state),
                Json(PublishRequest {
                    filename: "artifact.txt".to_string(),
                    data: "%%%not-base64%%%".to_string(),
                    project_id: "proj_test".to_string(),
                }),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["ok"], false);
        assert!(json["error"]
            .as_str()
            .unwrap_or_default()
            .starts_with("data must be valid base64:"));
    }
}
