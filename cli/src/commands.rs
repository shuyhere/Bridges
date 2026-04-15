use base64::Engine as _;

use crate::client_config::ClientConfig;
use crate::config::DaemonConfig;
use crate::identity;

/// Ensure the daemon is running. Auto-starts it if not.
pub fn ensure_daemon() {
    let port = std::env::var("BRIDGES_DAEMON_PORT").unwrap_or_else(|_| "7070".to_string());
    let url = format!("http://127.0.0.1:{}/status", port);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();

    if client.get(&url).send().is_ok() {
        return;
    }

    if crate::service::try_start_service_if_installed() {
        eprintln!("Starting daemon service...");
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if client.get(&url).send().is_ok() {
                return;
            }
        }
        eprintln!("Warning: daemon service started but not yet responding");
        return;
    }

    let exe = std::env::current_exe().unwrap_or_else(|_| "bridges".into());
    match std::process::Command::new(&exe)
        .arg("daemon")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => {
            eprintln!("Starting daemon...");
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if client.get(&url).send().is_ok() {
                    return;
                }
            }
            eprintln!("Warning: daemon started but not yet responding");
        }
        Err(e) => {
            eprintln!("Failed to start daemon: {}", e);
            std::process::exit(1);
        }
    }
}

/// One-command setup: generate keys, register, configure, start daemon.
pub fn cmd_setup(coordination: &str, runtime: &str, endpoint: &str, name: Option<&str>) {
    println!("=== Bridges Setup ===\n");

    // Derive display name: --name flag > system username > node_id
    let display_name = name
        .map(|n| n.to_string())
        .or_else(|| std::env::var("USER").ok())
        .or_else(|| std::env::var("USERNAME").ok());

    let (_signing_key, verifying_key) = identity::load_or_create_keypair();
    let node_id = identity::derive_node_id(&verifying_key);
    println!("Node ID: {}", node_id);
    if let Some(ref dn) = display_name {
        println!("Name: {}", dn);
    }

    println!("\nRegistering with {}...", coordination);
    cmd_register(coordination, display_name.as_deref());

    let cfg = DaemonConfig {
        coordination_url: coordination.to_string(),
        runtime: runtime.to_string(),
        runtime_endpoint: endpoint.to_string(),
        ..DaemonConfig::default()
    };
    let base = directories::BaseDirs::new().expect("cannot find home dir");
    let cfg_path = base.home_dir().join(".bridges").join("daemon.json");
    std::fs::create_dir_all(cfg_path.parent().unwrap()).ok();
    let json = serde_json::to_string_pretty(&cfg).unwrap();
    std::fs::write(&cfg_path, &json).expect("write daemon.json");
    println!("\nConfig written to {}", cfg_path.display());

    // Auto-install and start the daemon service
    println!("\nInstalling daemon service...");
    match crate::service::service_install() {
        Ok(msg) => println!("  {}", msg),
        Err(e) => {
            eprintln!(
                "  Service install failed: {} (you can start manually with: bridges daemon)",
                e
            );
        }
    }

    println!("\n=== Setup Complete ===");
    println!("  node:    {}", node_id);
    println!("  runtime: {}", runtime);
    println!("  server:  {}", coordination);
    println!("  daemon:  running (auto-restarts)");
    println!("\nNext steps:");
    println!("  bridges create my-project         # create a project");
    println!("  bridges invite -p <id>            # invite collaborators");
    println!("  bridges ask <node> \"hi\" -p <id>  # talk to a peer");
}

/// Register with a coordination server and save config.
pub fn cmd_register(coordination: &str, display_name: Option<&str>) {
    let (_signing_key, verifying_key) = identity::load_or_create_keypair();
    let node_id = identity::derive_node_id(&verifying_key);
    let ed_pub = bs58::encode(verifying_key.as_bytes()).into_string();
    let x_pub = hex::encode(
        crate::crypto::ed25519_to_x25519_public(verifying_key.as_bytes())
            .expect("own Ed25519 key must be valid"),
    );

    let name = display_name.unwrap_or(&node_id);

    let client = reqwest::blocking::Client::new();
    let body = serde_json::json!({
        "nodeId": node_id,
        "ed25519Pubkey": ed_pub,
        "x25519Pubkey": x_pub,
        "displayName": name,
    });

    let url = format!("{}/v1/auth/register", coordination.trim_end_matches('/'));
    let resp = match client.post(&url).json(&body).send() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to reach coordination server: {}", e);
            std::process::exit(1);
        }
    };

    if !resp.status().is_success() {
        eprintln!("Registration failed: HTTP {}", resp.status());
        std::process::exit(1);
    }

    let val: serde_json::Value = parse_json_or_exit(resp);
    let api_key = match val["apiKey"].as_str() {
        Some(k) => k.to_string(),
        None => {
            eprintln!("Server response missing apiKey field");
            std::process::exit(1);
        }
    };

    let cfg = ClientConfig {
        coordination: coordination.to_string(),
        node_id: node_id.clone(),
        api_key,
        display_name: Some(name.to_string()),
        owner: None,
    };
    cfg.save();
    println!("Registered as {}", node_id);
    println!("Config saved to ~/.bridges/config.json");
}

/// Create a project on the coordination server + local directory.
pub fn cmd_create(name: &str, description: Option<&str>) {
    let cfg = ClientConfig::load_or_exit();
    let client = authed_client(&cfg);
    let body = serde_json::json!({
        "slug": name,
        "displayName": name,
        "description": description,
    });
    let url = format!("{}/v1/projects", cfg.coordination);
    let resp = send_or_exit(&client, &url, Some(&body), "POST");
    if !resp.status().is_success() {
        if resp.status().as_u16() == 409 {
            eprintln!(
                "Project '{}' already exists. Use a different name, or invite collaborators with:",
                name
            );
            eprintln!("  bridges invite -p <project_id>");
            eprintln!("Run 'bridges status' to see your existing projects.");
        } else {
            eprintln!("Create failed: HTTP {}", resp.status());
        }
        std::process::exit(1);
    }
    let val: serde_json::Value = parse_json_or_exit(resp);
    let project_id = val["projectId"].as_str().unwrap_or("?");

    // Create local project directory at ~/bridges-projects/<slug>/
    let project_dir = crate::queries::project_dir_for_slug(name);
    std::fs::create_dir_all(&project_dir).ok();

    // Initialize local workspace metadata and optional shared workspace files.
    crate::workspace::init_workspace(&project_dir, name);
    crate::sync_engine::init_shared(&project_dir);

    // Store in local DB with path
    let conn = crate::db::open_db();
    crate::db::init_db(&conn);
    crate::queries::insert_project(
        &conn,
        &crate::models::Project {
            project_id: project_id.to_string(),
            slug: name.to_string(),
            display_name: Some(name.to_string()),
            description: description.map(|d| d.to_string()),
            project_path: Some(project_dir.to_string_lossy().to_string()),
            owner_principal_id: None,
            status: "active".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        },
    );

    // Write initial MEMBERS.md (creator as owner)
    let now = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let (_, vk) = identity::load_or_create_keypair();
    let my_node = identity::derive_node_id(&vk);
    crate::sync_engine::update_members(&project_dir, &[(my_node, "owner".to_string(), now)]);

    println!("Project created: {}", project_id);
    println!("  path: {}", project_dir.display());
}

/// Generate an invite token for a project.
pub fn cmd_invite(project_id: &str) {
    require_project_id(project_id);
    let cfg = ClientConfig::load_or_exit();
    let client = authed_client(&cfg);
    let url = format!("{}/v1/projects/{}/invites", cfg.coordination, project_id);
    let body = serde_json::json!({ "maxUses": 10 });
    let resp = send_or_exit(&client, &url, Some(&body), "POST");
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        eprintln!("Invite failed: HTTP {} — {}", status, text);
        eprintln!("URL: {}", url);
        std::process::exit(1);
    }
    let val: serde_json::Value = parse_json_or_exit(resp);
    println!(
        "Invite token: {}",
        val["inviteToken"].as_str().unwrap_or("?")
    );
}

/// Join a project with an invite token + create local directory.
pub fn cmd_join(invite_token: &str, project_id: &str) {
    require_project_id(project_id);
    let cfg = ClientConfig::load_or_exit();
    let client = authed_client(&cfg);
    let url = format!("{}/v1/projects/{}/join", cfg.coordination, project_id);
    let body = serde_json::json!({
        "inviteToken": invite_token,
        "agentRole": "member",
    });
    let resp = send_or_exit(&client, &url, Some(&body), "POST");
    if !resp.status().is_success() {
        eprintln!("Join failed: HTTP {}", resp.status());
        std::process::exit(1);
    }

    // Fetch project details to get the slug.
    let details_url = format!("{}/v1/projects/{}", cfg.coordination, project_id);
    let slug = match client.get(&details_url).send() {
        Ok(resp) if resp.status().is_success() => {
            let val: serde_json::Value = resp.json().unwrap_or_default();
            val["slug"].as_str().unwrap_or(project_id).to_string()
        }
        _ => project_id.replace("proj_", ""),
    };

    // Check if project directory already exists locally
    let conn = crate::db::open_db();
    crate::db::init_db(&conn);
    let project_dir = if let Some(existing) = crate::queries::get_project_path_by_slug(&conn, &slug)
    {
        std::path::PathBuf::from(existing)
    } else {
        let dir = crate::queries::project_dir_for_slug(&slug);
        std::fs::create_dir_all(&dir).ok();
        dir
    };

    // Initialize local workspace metadata and optional shared workspace files.
    crate::workspace::init_workspace(&project_dir, &slug);
    crate::sync_engine::init_shared(&project_dir);

    // Store in local DB
    crate::queries::insert_project(
        &conn,
        &crate::models::Project {
            project_id: project_id.to_string(),
            slug: slug.clone(),
            display_name: Some(slug.clone()),
            description: None,
            project_path: Some(project_dir.to_string_lossy().to_string()),
            owner_principal_id: None,
            status: "active".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        },
    );

    // Fetch and write MEMBERS.md
    let members_url = format!("{}/v1/projects/{}/members", cfg.coordination, project_id);
    if let Ok(resp) = client.get(&members_url).send() {
        if let Ok(members) = resp.json::<Vec<serde_json::Value>>() {
            let member_list: Vec<(String, String, String)> = members
                .iter()
                .map(|m| {
                    let nid = m["nodeId"].as_str().unwrap_or("?").to_string();
                    let role = m["agentRole"].as_str().unwrap_or("member").to_string();
                    let joined = m["joinedAt"].as_str().unwrap_or("?").to_string();
                    (nid, role, joined)
                })
                .collect();
            crate::sync_engine::update_members(&project_dir, &member_list);
        }
    }

    println!("Joined project {}", project_id);
    println!("  path: {}", project_dir.display());
}

/// List members of a project.
pub fn cmd_members(project_id: &str) {
    require_project_id(project_id);
    let cfg = ClientConfig::load_or_exit();
    let client = authed_client(&cfg);
    let url = format!("{}/v1/projects/{}/members", cfg.coordination, project_id);
    let resp = send_or_exit(&client, &url, None, "GET");
    if !resp.status().is_success() {
        eprintln!("Members failed: HTTP {}", resp.status());
        std::process::exit(1);
    }
    let members: Vec<serde_json::Value> = parse_json_or_exit(resp);
    println!("Members of {}:", project_id);
    for m in &members {
        let name = m["displayName"].as_str().unwrap_or("?");
        let role = m["agentRole"].as_str().unwrap_or("member");
        let nid = m["nodeId"].as_str().unwrap_or("?");
        println!("  {} ({}) [{}]", nid, name, role);
    }
}

/// Get the daemon local API URL.
fn daemon_url() -> String {
    let port = std::env::var("BRIDGES_DAEMON_PORT").unwrap_or_else(|_| "7070".to_string());
    format!("http://127.0.0.1:{}", port)
}

/// Poll the daemon for a response by request_id. Blocks until response or timeout.
fn poll_response(request_id: &str, timeout_secs: u64) -> Option<(String, String)> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let url = format!("{}/response/{}", daemon_url(), request_id);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

    while std::time::Instant::now() < deadline {
        if let Ok(resp) = client.get(&url).send() {
            if let Ok(val) = resp.json::<serde_json::Value>() {
                if val["ready"].as_bool() == Some(true) {
                    let from = val["from_node"].as_str().unwrap_or("?").to_string();
                    let text = val["response"].as_str().unwrap_or("").to_string();
                    return Some((from, text));
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    None
}

/// Resolve project directory from project ID in local DB.
fn resolve_project_dir(project_id: &str) -> Option<std::path::PathBuf> {
    let conn = crate::db::open_db();
    crate::db::init_db(&conn);
    crate::queries::get_project_path(&conn, project_id).map(std::path::PathBuf::from)
}

fn require_local_project_dir(project_id: &str) -> std::path::PathBuf {
    resolve_project_dir(project_id).unwrap_or_else(|| {
        eprintln!("Unknown local project: {}", project_id);
        eprintln!("Join or create the project on this machine first.");
        std::process::exit(1);
    })
}

pub fn cmd_session_list(project_id: &str, peer_id: &str) {
    require_project_id(project_id);
    let project_dir = require_local_project_dir(project_id);
    let project_dir = project_dir.to_string_lossy().to_string();
    let sessions =
        crate::conversation_memory::list_sessions(&project_dir, peer_id).unwrap_or_else(|e| {
            eprintln!("Session list failed: {}", e);
            std::process::exit(1);
        });

    if sessions.is_empty() {
        println!("No conversation sessions for {} in {}", peer_id, project_id);
        return;
    }

    println!("Sessions for {} in {}:", peer_id, project_id);
    for session in sessions {
        let active = if session.active { " [active]" } else { "" };
        let summary = if session.has_summary { " yes" } else { " no" };
        let updated = session
            .last_timestamp
            .unwrap_or_else(|| "never".to_string());
        println!(
            "  {}{}  exchanges={}  summary={}  updated={}",
            session.session_id, active, session.exchange_count, summary, updated
        );
    }
}

pub fn cmd_session_new(project_id: &str, peer_id: &str) {
    require_project_id(project_id);
    let project_dir = require_local_project_dir(project_id);
    let session_id =
        crate::conversation_memory::create_session(&project_dir.to_string_lossy(), peer_id)
            .unwrap_or_else(|e| {
                eprintln!("Session create failed: {}", e);
                std::process::exit(1);
            });
    println!("Created new active session for {}:", peer_id);
    println!("  {}", session_id);
}

pub fn cmd_session_use(project_id: &str, peer_id: &str, session_id: &str) {
    require_project_id(project_id);
    let project_dir = require_local_project_dir(project_id);
    crate::conversation_memory::use_session(&project_dir.to_string_lossy(), peer_id, session_id)
        .unwrap_or_else(|e| {
            eprintln!("Session switch failed: {}", e);
            std::process::exit(1);
        });
    println!("Active session for {} set to {}", peer_id, session_id);
}

pub fn cmd_session_reset(project_id: &str, peer_id: &str, session_id: Option<&str>, all: bool) {
    require_project_id(project_id);
    let project_dir = require_local_project_dir(project_id);
    let project_dir = project_dir.to_string_lossy().to_string();

    if all {
        crate::conversation_memory::reset_all_sessions(&project_dir, peer_id).unwrap_or_else(|e| {
            eprintln!("Session reset failed: {}", e);
            std::process::exit(1);
        });
        println!("Reset all sessions for {} in {}", peer_id, project_id);
        return;
    }

    let session_id = session_id.unwrap_or_else(|| {
        eprintln!("Provide --session <id> or use --all");
        std::process::exit(1);
    });
    crate::conversation_memory::reset_session(&project_dir, peer_id, session_id).unwrap_or_else(
        |e| {
            eprintln!("Session reset failed: {}", e);
            std::process::exit(1);
        },
    );
    println!(
        "Reset session {} for {} in {}",
        session_id, peer_id, project_id
    );
}

/// Ask another agent a question — sends E2E encrypted and waits for a response.
pub fn cmd_ask(node_id: &str, question: &str, project_id: Option<&str>, new_session: bool) {
    ensure_daemon();

    let pid = project_id.unwrap_or("");

    let client = reqwest::blocking::Client::new();
    let url = format!("{}/ask", daemon_url());
    let body = serde_json::json!({
        "node_id": node_id,
        "question": question,
        "project_id": pid,
        "new_session": new_session,
    });
    let resp = match client.post(&url).json(&body).send() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Daemon unreachable: {}", e);
            std::process::exit(1);
        }
    };
    if !resp.status().is_success() {
        eprintln!("Ask failed: HTTP {}", resp.status());
        std::process::exit(1);
    }
    let val: serde_json::Value = parse_json_or_exit(resp);
    if val["ok"].as_bool() != Some(true) {
        eprintln!("Ask failed: {}", val["error"].as_str().unwrap_or("unknown"));
        std::process::exit(1);
    }

    let request_id = match val["request_id"].as_str() {
        Some(id) => id,
        None => {
            println!("Sent (E2E encrypted)");
            return;
        }
    };

    eprintln!("Waiting for response from {}...", node_id);
    match poll_response(request_id, 120) {
        Some((from, text)) => {
            println!("[Response from {}]\n{}", from, text);
        }
        None => {
            eprintln!("Timeout: no response received within 120 seconds");
            std::process::exit(1);
        }
    }
}

/// Start a debate — sends to all members and collects responses.
pub fn cmd_debate(topic: &str, project_id: &str, new_session: bool) {
    ensure_daemon();
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/debate", daemon_url());
    let body = serde_json::json!({
        "topic": topic,
        "project_id": project_id,
        "new_session": new_session,
    });
    let resp = match client.post(&url).json(&body).send() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Daemon unreachable: {}", e);
            std::process::exit(1);
        }
    };
    if !resp.status().is_success() {
        eprintln!("Debate failed: HTTP {}", resp.status());
        std::process::exit(1);
    }
    let val: serde_json::Value = parse_json_or_exit(resp);
    let sent_to = val["sent_to"].as_array().map(|a| a.len()).unwrap_or(0);
    if sent_to == 0 {
        println!("No members to debate with");
        return;
    }

    let request_ids: Vec<String> = val["request_ids"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if request_ids.is_empty() {
        println!("Debate sent to {} members (E2E encrypted)", sent_to);
        return;
    }

    eprintln!("Waiting for {} responses...", request_ids.len());
    for request_id in &request_ids {
        match poll_response(request_id, 120) {
            Some((from, text)) => {
                println!("\n[Response from {}]\n{}", from, text);
            }
            None => {
                eprintln!("Timeout waiting for response to {}", request_id);
            }
        }
    }
}

/// Broadcast a message to all project members — routed through local daemon (E2E encrypted).
pub fn cmd_broadcast(message: &str, project_id: &str) {
    ensure_daemon();
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/broadcast", daemon_url());
    let body = serde_json::json!({
        "message": message,
        "project_id": project_id,
        "message_type": "broadcast",
    });
    let resp = match client.post(&url).json(&body).send() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Daemon unreachable: {}", e);
            std::process::exit(1);
        }
    };
    if !resp.status().is_success() {
        eprintln!("Broadcast failed: HTTP {}", resp.status());
        std::process::exit(1);
    }
    let val: serde_json::Value = parse_json_or_exit(resp);
    let targets = val["sent_to"].as_array().map(|a| a.len()).unwrap_or(0);
    println!("Broadcast sent to {} members (E2E encrypted)", targets);
}

/// Publish a file as an artifact to all project members — routed through local daemon (E2E encrypted).
pub fn cmd_publish(file: &str, project_id: &str) {
    ensure_daemon();
    let data = std::fs::read(file).unwrap_or_else(|e| {
        eprintln!("Cannot read {}: {}", file, e);
        std::process::exit(1);
    });
    let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
    let filename = std::path::Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");

    let client = reqwest::blocking::Client::new();
    let url = format!("{}/publish", daemon_url());
    let body = serde_json::json!({
        "filename": filename,
        "data": encoded,
        "project_id": project_id,
    });
    let resp = match client.post(&url).json(&body).send() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Daemon unreachable: {}", e);
            std::process::exit(1);
        }
    };
    if !resp.status().is_success() {
        eprintln!("Publish failed: HTTP {}", resp.status());
        std::process::exit(1);
    }
    println!(
        "Published {} to project {} (E2E encrypted)",
        filename, project_id
    );
}

/// Build a blocking reqwest client with Bearer auth.
fn authed_client(cfg: &ClientConfig) -> reqwest::blocking::Client {
    use reqwest::header;
    let mut headers = header::HeaderMap::new();
    let val = format!("Bearer {}", cfg.api_key);
    headers.insert(
        header::AUTHORIZATION,
        header::HeaderValue::from_str(&val).unwrap(),
    );
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

/// Send a blocking HTTP request or exit on network error.
fn send_or_exit(
    client: &reqwest::blocking::Client,
    url: &str,
    body: Option<&serde_json::Value>,
    method: &str,
) -> reqwest::blocking::Response {
    let req = match method {
        "GET" => client.get(url),
        _ => {
            let mut r = client.post(url);
            if let Some(b) = body {
                r = r.json(b);
            }
            r
        }
    };
    match req.send() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Request to {} failed: {}", url, e);
            std::process::exit(1);
        }
    }
}

/// Parse a JSON response or exit on parse error.
fn parse_json_or_exit<T: serde::de::DeserializeOwned>(resp: reqwest::blocking::Response) -> T {
    match resp.json::<T>() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Invalid response from server: {}", e);
            std::process::exit(1);
        }
    }
}

fn require_project_id(project_id: &str) {
    if !project_id.starts_with("proj_") {
        eprintln!(
            "Project must be a project ID like proj_xxx, not a slug/name: {}",
            project_id
        );
        std::process::exit(1);
    }
}
