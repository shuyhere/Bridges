use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::config::DaemonConfig;
use crate::connmgr::ConnManager;
use crate::coord_client::{CoordClient, EndpointHint, MemberInfo};
use crate::derp_client::DerpClient;
use crate::identity;
use crate::listener::dispatch;
use crate::local_api;
use crate::mdns;
use crate::stun;
use crate::transport::Transport;

fn resolve_runtime_project_dir(project_id: &str, fallback: &str) -> String {
    if !project_id.is_empty() {
        if let Ok(conn) = crate::db::open_db() {
            if crate::db::init_db(&conn).is_ok() {
                if let Some(path) = crate::queries::get_project_path(&conn, project_id) {
                    return path;
                }
            }
        }
    }
    fallback.to_string()
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_inbound_message(
    coord: &CoordClient,
    runtime_type: &str,
    runtime_endpoint: &str,
    fallback_project_dir: &str,
    from: &str,
    project_id: &str,
    kind: &str,
    session_id: Option<&str>,
    payload: &serde_json::Value,
) -> Result<String, String> {
    let project_dir = resolve_runtime_project_dir(project_id, fallback_project_dir);
    let runtime = dispatch::create_runtime(runtime_type, runtime_endpoint, &project_dir)?;
    let sender = if project_id.is_empty() {
        None
    } else {
        coord
            .get_project_members(project_id)
            .await
            .ok()
            .and_then(|members| {
                members
                    .into_iter()
                    .find(|member: &MemberInfo| member.node_id == from)
            })
    };
    let sandbox = dispatch::create_sandbox(
        &project_dir,
        from,
        sender
            .as_ref()
            .and_then(|member| member.display_name.as_deref()),
        sender.as_ref().and_then(|member| member.role.as_deref()),
        project_id,
        kind,
        session_id,
        payload,
    );
    let response = dispatch::dispatch_message(runtime.as_ref(), &sandbox).await?;
    let _ = crate::conversation_memory::append_exchange(
        &project_dir,
        from,
        session_id,
        kind,
        &sandbox.query,
        &response,
    );
    Ok(response)
}

async fn decode_mailbox_blob(
    coord: &CoordClient,
    my_node_id: &str,
    my_x25519_priv: &[u8; 32],
    from_node_id: &str,
    project_id: Option<&str>,
    blob: &str,
) -> Result<Vec<u8>, String> {
    let keys = if let Some(project_id) = project_id {
        coord
            .get_peer_keys_in_project(from_node_id, project_id)
            .await?
    } else {
        coord.get_peer_keys(from_node_id).await?
    };
    let decoded = hex::decode(&keys.x25519_pub).map_err(|e| format!("bad x25519 pubkey: {}", e))?;
    if decoded.len() != 32 {
        return Err("x25519 pubkey wrong length".to_string());
    }
    let mut x_pub = [0u8; 32];
    x_pub.copy_from_slice(&decoded);
    match crate::crypto::decrypt_mailbox_payload(
        my_node_id,
        from_node_id,
        my_x25519_priv,
        &x_pub,
        blob,
    ) {
        Ok(plaintext) => Ok(plaintext),
        Err(encrypted_err) => {
            use base64::Engine;
            let plaintext = base64::engine::general_purpose::STANDARD
                .decode(blob)
                .map_err(|_| format!("mailbox decrypt failed: {}", encrypted_err))?;
            eprintln!(
                "  mailbox warning: accepted legacy plaintext relay from {}",
                from_node_id
            );
            Ok(plaintext)
        }
    }
}

async fn encode_mailbox_blob(
    coord: &CoordClient,
    from_node_id: &str,
    my_x25519_priv: &[u8; 32],
    target_node_id: &str,
    project_id: Option<&str>,
    plaintext: &[u8],
) -> Result<String, String> {
    let keys = if let Some(project_id) = project_id {
        coord
            .get_peer_keys_in_project(target_node_id, project_id)
            .await?
    } else {
        coord.get_peer_keys(target_node_id).await?
    };
    let decoded = hex::decode(&keys.x25519_pub).map_err(|e| format!("bad x25519 pubkey: {}", e))?;
    if decoded.len() != 32 {
        return Err("x25519 pubkey wrong length".to_string());
    }
    let mut x_pub = [0u8; 32];
    x_pub.copy_from_slice(&decoded);
    crate::crypto::encrypt_mailbox_payload(
        from_node_id,
        target_node_id,
        my_x25519_priv,
        &x_pub,
        plaintext,
    )
}

/// Run the Bridges daemon. Blocks until interrupted.
pub async fn run(_foreground: bool) -> Result<(), String> {
    let (signing_key, verifying_key) =
        identity::load_or_create_keypair().map_err(|err| format!("load identity: {}", err))?;
    let node_id = identity::derive_node_id(&verifying_key);
    let keypair = identity::NodeKeypair {
        signing: signing_key.clone(),
    };

    let cfg = DaemonConfig::load().map_err(|err| format!("load daemon config: {}", err))?;
    println!("Bridges daemon starting: {}", node_id);

    // Derive X25519 keys
    let x_priv = identity::x25519_private_key(&keypair);
    let api_key = cfg
        .api_key()
        .map_err(|err| format!("load API key: {}", err))?;
    if api_key.is_empty() {
        return Err("Not registered. Run `bridges setup` or `bridges register` first.".to_string());
    }

    // Connect to coordination server using the existing API key from ~/.bridges/config.json.
    // Do not call /v1/auth/register here: that endpoint rotates the node's API key.
    let coord = CoordClient::new(&cfg.coordination_url, &api_key);
    println!("  coordination auth: configured API key");

    // STUN: discover reflexive address
    let mut hints: Vec<EndpointHint> = Vec::new();
    for server in &cfg.stun_servers {
        match stun::get_reflexive_addr(server).await {
            Ok(addr) => {
                println!("  reflexive address: {}", addr);
                hints.push(EndpointHint {
                    addr: addr.to_string(),
                    hint_type: "stun".to_string(),
                });
            }
            Err(e) => eprintln!("  STUN {} failed: {}", server, e),
        }
    }
    if !hints.is_empty() {
        coord.push_endpoint_hints(&hints).await.ok();
    }

    // mDNS announcement
    mdns::announce(&node_id, cfg.local_api_port);

    // DERP WebSocket
    let derp = if cfg.derp_enabled {
        let derp_url = format!("{}/ws/derp", cfg.coordination_url);
        match DerpClient::connect(&derp_url, &api_key).await {
            Ok(client) => {
                println!("  DERP relay connected");
                Some(client)
            }
            Err(e) => {
                eprintln!("  DERP connect failed: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Build transport
    let conn_mgr = ConnManager::new(Some(coord.clone()));
    let transport = Transport::new(conn_mgr, derp, node_id.clone(), x_priv).await?;
    let transport = Arc::new(transport);

    println!("  runtime: {} ({})", cfg.runtime, cfg.runtime);

    // Shared response store (read by CLI via /response/:id endpoint)
    let responses: Arc<Mutex<HashMap<String, local_api::PendingResponse>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Local API state
    let api_state = Arc::new(local_api::ApiState {
        transport: transport.clone(),
        coord: Arc::new(coord.clone()),
        node_id: node_id.clone(),
        my_x25519_priv: x_priv,
        responses: responses.clone(),
    });

    // Start local API server
    let api_port = cfg.local_api_port;
    let api_handle = tokio::spawn(async move {
        if let Err(e) = local_api::serve(api_state, api_port).await {
            eprintln!("Local API error: {}", e);
        }
    });

    // Clone config values needed by both loops
    let project_dir_for_poll = cfg.project_dir.clone();
    let runtime_type = cfg.runtime.clone();
    let runtime_endpoint = cfg.runtime_endpoint.clone();

    // Inbound message loop
    let recv_transport = transport.clone();
    let recv_node_id = node_id.clone();
    let recv_responses = responses.clone();
    let recv_runtime_type = runtime_type.clone();
    let recv_runtime_endpoint = runtime_endpoint.clone();
    let recv_project_dir = cfg.project_dir.clone();
    let recv_coord = coord.clone();
    let recv_x_priv = x_priv;
    let recv_handle = tokio::spawn(async move {
        loop {
            match recv_transport.recv().await {
                Ok((source, plaintext)) => {
                    let peer_id = source.node_id().to_string();
                    let msg: serde_json::Value = match serde_json::from_slice(&plaintext) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("invalid message from {}: {}", peer_id, e);
                            continue;
                        }
                    };

                    let from = msg["from"].as_str().unwrap_or(&peer_id);
                    let project_id = msg["projectId"].as_str().unwrap_or("");
                    let kind = msg["messageType"].as_str().unwrap_or("ask");
                    let request_id = msg["requestId"].as_str().unwrap_or("");
                    let session_id = msg["sessionId"].as_str();
                    let payload = &msg["payload"];

                    // Shared-workspace sync is handled out of band, not through relay messages.
                    if kind == "sync" {
                        println!(
                            "  sync message from {} (handled out of band, skipping)",
                            from
                        );
                        continue;
                    }

                    // Handle response messages — store them for the CLI to poll
                    if kind == "response" {
                        let response_text = payload["message"].as_str().unwrap_or("");
                        if !request_id.is_empty() {
                            local_api::store_response(
                                &recv_responses,
                                request_id,
                                from,
                                response_text,
                            )
                            .await;
                            println!(
                                "  response from {} for {} ({} chars)",
                                from,
                                request_id,
                                response_text.len()
                            );
                        } else {
                            println!(
                                "  response from {} (no request_id, {} chars)",
                                from,
                                response_text.len()
                            );
                        }
                        continue;
                    }

                    match dispatch_inbound_message(
                        &recv_coord,
                        &recv_runtime_type,
                        &recv_runtime_endpoint,
                        &recv_project_dir,
                        from,
                        project_id,
                        kind,
                        session_id,
                        payload,
                    )
                    .await
                    {
                        Ok(response) => {
                            println!(
                                "  {} from {} → responded ({} chars)",
                                kind,
                                from,
                                response.len()
                            );
                            // Send encrypted response back, include requestId so sender can match it
                            let reply = serde_json::json!({
                                "from": recv_node_id,
                                "projectId": project_id,
                                "messageType": "response",
                                "requestId": request_id,
                                "payload": { "message": response },
                            });
                            let reply_bytes = serde_json::to_vec(&reply).unwrap_or_default();
                            if let Err(e) = recv_transport.send(from, &reply_bytes).await {
                                eprintln!("  failed to send reply to {}: {}", from, e);
                                match encode_mailbox_blob(
                                    &recv_coord,
                                    &recv_node_id,
                                    &recv_x_priv,
                                    from,
                                    Some(project_id),
                                    &reply_bytes,
                                )
                                .await
                                {
                                    Ok(reply_blob) => {
                                        if let Err(relay_err) = recv_coord
                                            .relay_message(from, &reply_blob, Some(project_id))
                                            .await
                                        {
                                            eprintln!(
                                                "  failed to relay reply to {}: {}",
                                                from, relay_err
                                            );
                                        }
                                    }
                                    Err(encode_err) => eprintln!(
                                        "  failed to encrypt relay reply to {}: {}",
                                        from, encode_err
                                    ),
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("  dispatch error for {} from {}: {}", kind, from, e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("recv error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    });

    // Mailbox polling loop — fetch messages from coordination server every 5s
    let poll_coord = coord.clone();
    let poll_responses = responses.clone();
    let poll_node_id = node_id.clone();
    let poll_project_dir = project_dir_for_poll;
    let poll_runtime_type = runtime_type.clone();
    let poll_runtime_endpoint = runtime_endpoint.clone();
    let poll_x_priv = x_priv;
    let poll_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let messages = match poll_coord.fetch_mailbox().await {
                Ok(m) if !m.is_empty() => m,
                _ => continue,
            };

            println!("  mailbox: {} pending messages", messages.len());
            for msg in &messages {
                let from = msg["from"].as_str().unwrap_or("");
                let blob = msg["blob"].as_str().unwrap_or("");
                let mailbox_project_id = msg["projectId"].as_str();
                if blob.is_empty() {
                    continue;
                }

                let plaintext = match decode_mailbox_blob(
                    &poll_coord,
                    &poll_node_id,
                    &poll_x_priv,
                    from,
                    mailbox_project_id,
                    blob,
                )
                .await
                {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("  mailbox decode failed from {}: {}", from, e);
                        continue;
                    }
                };

                let parsed: serde_json::Value = match serde_json::from_slice(&plaintext) {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!("  mailbox: bad json from {}", from);
                        continue;
                    }
                };

                let msg_from = parsed["from"].as_str().unwrap_or(from);
                let _project_id = parsed["projectId"].as_str().unwrap_or("");
                let kind = parsed["messageType"].as_str().unwrap_or("ask");
                let request_id = parsed["requestId"].as_str().unwrap_or("");
                let session_id = parsed["sessionId"].as_str();
                let payload = &parsed["payload"];

                // Shared-workspace sync is handled out of band, not through mailbox.
                if kind == "sync" {
                    continue;
                }

                // Handle response
                if kind == "response" {
                    let response_text = payload["message"].as_str().unwrap_or("");
                    if !request_id.is_empty() {
                        local_api::store_response(
                            &poll_responses,
                            request_id,
                            msg_from,
                            response_text,
                        )
                        .await;
                        println!(
                            "  mailbox response from {} ({} chars)",
                            msg_from,
                            response_text.len()
                        );
                    }
                    continue;
                }

                match dispatch_inbound_message(
                    &poll_coord,
                    &poll_runtime_type,
                    &poll_runtime_endpoint,
                    &poll_project_dir,
                    msg_from,
                    _project_id,
                    kind,
                    session_id,
                    payload,
                )
                .await
                {
                    Ok(response) => {
                        println!(
                            "  mailbox {} from {} → responded ({} chars)",
                            kind,
                            msg_from,
                            response.len()
                        );
                        // Send response back via relay
                        let reply = serde_json::json!({
                            "from": poll_node_id,
                            "messageType": "response",
                            "requestId": request_id,
                            "payload": { "message": response },
                        });
                        match encode_mailbox_blob(
                            &poll_coord,
                            &poll_node_id,
                            &poll_x_priv,
                            msg_from,
                            Some(_project_id),
                            &serde_json::to_vec(&reply).unwrap_or_default(),
                        )
                        .await
                        {
                            Ok(reply_blob) => {
                                if let Err(e) = poll_coord
                                    .relay_message(msg_from, &reply_blob, Some(_project_id))
                                    .await
                                {
                                    eprintln!("  failed to relay response to {}: {}", msg_from, e);
                                }
                            }
                            Err(e) => eprintln!(
                                "  failed to encrypt relay response to {}: {}",
                                msg_from, e
                            ),
                        }
                    }
                    Err(e) => eprintln!(
                        "  mailbox dispatch error for {} from {}: {}",
                        kind, msg_from, e
                    ),
                }
            }
        }
    });

    println!("Bridges daemon running. Press Ctrl+C to stop.");

    tokio::signal::ctrl_c()
        .await
        .map_err(|e| format!("signal: {}", e))?;

    println!("Shutting down daemon.");
    poll_handle.abort();
    api_handle.abort();
    recv_handle.abort();
    Ok(())
}
