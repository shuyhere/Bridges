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
pub fn cmd_setup(
    coordination: &str,
    token: Option<&str>,
    runtime: &str,
    endpoint: &str,
    name: Option<&str>,
) {
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

    if let Some(api_token) = token {
        // Token-based setup: use a coordination-service API token
        println!("\nRegistering node with API token...");
        cmd_register_with_token(coordination, api_token, display_name.as_deref());
    } else {
        // Legacy setup: register directly (creates a new node-level API key)
        println!("\nRegistering with {}...", coordination);
        cmd_register(coordination, display_name.as_deref());
    }

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

/// Register a node using a coordination-service API token.
/// The token is used as Bearer auth and may also become the node's API key.
fn cmd_register_with_token(coordination: &str, api_token: &str, display_name: Option<&str>) {
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

    // Register node with the provided token as Bearer auth
    let url = format!("{}/v1/auth/register", coordination.trim_end_matches('/'));
    let resp = match client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_token))
        .json(&body)
        .send()
    {
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
    // The server returns a fresh node-level API key
    let node_api_key = match val["apiKey"].as_str() {
        Some(k) => k.to_string(),
        None => {
            // If server didn't return an apiKey, use the provided token directly
            api_token.to_string()
        }
    };

    let gitea_user = val["giteaUser"].as_str().map(|s| s.to_string());
    let gitea_token = val["giteaToken"].as_str().map(|s| s.to_string());
    let gitea_password = val["giteaPassword"].as_str().map(|s| s.to_string());
    let gitea_url = val["giteaUrl"].as_str().map(|server_gitea_url| {
        let coord_host = coordination
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split(':')
            .next()
            .unwrap_or("127.0.0.1");
        let gitea_port = server_gitea_url
            .split(':')
            .next_back()
            .unwrap_or("3000")
            .trim_end_matches('/');
        format!("http://{}:{}", coord_host, gitea_port)
    });

    if gitea_user.is_some() {
        println!("Gitea account: {}", gitea_user.as_deref().unwrap_or("?"));
    }

    let cfg = ClientConfig {
        coordination: coordination.to_string(),
        node_id: node_id.clone(),
        api_key: node_api_key,
        display_name: Some(name.to_string()),
        owner: None,
        gitea_url,
        gitea_user,
        gitea_token,
        gitea_password,
    };
    cfg.save();
    println!("Registered as {}", node_id);
    println!("Config saved to ~/.bridges/config.json");
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

    let gitea_user = val["giteaUser"].as_str().map(|s| s.to_string());
    let gitea_token = val["giteaToken"].as_str().map(|s| s.to_string());
    let gitea_password = val["giteaPassword"].as_str().map(|s| s.to_string());

    // Derive Gitea external URL from coordination server host + Gitea port
    let gitea_url = val["giteaUrl"].as_str().map(|server_gitea_url| {
        let coord_host = coordination
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split(':')
            .next()
            .unwrap_or("127.0.0.1");
        let gitea_port = server_gitea_url
            .split(':')
            .next_back()
            .unwrap_or("3000")
            .trim_end_matches('/');
        format!("http://{}:{}", coord_host, gitea_port)
    });

    if gitea_user.is_some() {
        println!("Gitea account: {}", gitea_user.as_deref().unwrap_or("?"));
        if let Some(ref url) = gitea_url {
            println!(
                "Gitea URL: {} (credentials saved to ~/.bridges/config.json)",
                url
            );
        }
    } else {
        eprintln!(
            "Warning: server did not provide Gitea credentials. Coordination works, but repo sync will not."
        );
        eprintln!(
            "Check the server logs for 'Gitea setup failed', 'Gitea integration: disabled', or 'Gitea account creation failed'."
        );
    }

    let cfg = ClientConfig {
        coordination: coordination.to_string(),
        node_id: node_id.clone(),
        api_key,
        display_name: Some(name.to_string()),
        owner: None,
        gitea_url,
        gitea_user,
        gitea_token,
        gitea_password,
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
        "giteaOwner": cfg.gitea_user,
        "giteaRepo": name,
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

    // Init .bridges/ and .shared/
    crate::workspace::init_workspace(&project_dir, name);
    crate::sync_engine::init_shared(&project_dir);

    // Init git repo with "main" as default branch
    if let Err(e) = crate::sync_engine::git_init(&project_dir) {
        eprintln!("  git init failed: {}", e);
    }
    crate::sync_engine::git_commit(&project_dir, "init project").ok();

    // Create Gitea repo under user's account and set remote
    if let (Some(gitea_url), Some(gitea_token), Some(gitea_user)) =
        (&cfg.gitea_url, &cfg.gitea_token, &cfg.gitea_user)
    {
        if let Err(e) = crate::sync_engine::gitea_create_user_repo(gitea_url, gitea_token, name) {
            eprintln!("  Gitea repo creation failed: {}", e);
        }
        // Remote URL stays clean; git auth is injected per command.
        let remote_url = format!(
            "http://{host}/{gitea_user}/{repo}.git",
            gitea_user = gitea_user,
            host = gitea_url
                .trim_start_matches("http://")
                .trim_start_matches("https://"),
            repo = name,
        );
        if let Err(e) = crate::sync_engine::git_add_remote(&project_dir, &remote_url) {
            eprintln!("  Git remote setup failed: {}", e);
        }
        let branch = crate::sync_engine::git_current_branch(&project_dir);
        if let Err(e) = crate::sync_engine::git_push(&project_dir, &branch) {
            eprintln!("  Git push failed: {}", e);
        } else {
            println!("  gitea: {}/{}/{}", gitea_url, gitea_user, name);
        }
    } else {
        eprintln!("  Gitea not configured — this client has no Gitea credentials.");
        eprintln!("  Run `bridges setup` after verifying the server started with Gitea enabled.");
        eprintln!(
            "  If setup still omits Gitea info, check the server logs for 'Gitea setup failed' or 'Gitea integration: disabled'."
        );
    }

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

    // Fetch project details to get the slug and owner
    let details_url = format!("{}/v1/projects/{}", cfg.coordination, project_id);
    let (slug, owner_node_id, repo_owner, repo_name) = match client.get(&details_url).send() {
        Ok(resp) if resp.status().is_success() => {
            let val: serde_json::Value = resp.json().unwrap_or_default();
            let s = val["slug"].as_str().unwrap_or(project_id).to_string();
            let o = val["createdBy"].as_str().unwrap_or("").to_string();
            let repo_owner = val["giteaOwner"].as_str().unwrap_or("").to_string();
            let repo_name = val["giteaRepo"].as_str().unwrap_or(&s).to_string();
            (s, o, repo_owner, repo_name)
        }
        _ => (
            project_id.replace("proj_", ""),
            String::new(),
            String::new(),
            String::new(),
        ),
    };

    // Get owner's display name (Gitea username) from members list
    let owner_gitea_user = if !repo_owner.is_empty() {
        repo_owner
    } else if !owner_node_id.is_empty() {
        let members_url = format!("{}/v1/projects/{}/members", cfg.coordination, project_id);
        client
            .get(&members_url)
            .send()
            .ok()
            .and_then(|r| r.json::<Vec<serde_json::Value>>().ok())
            .and_then(|members| {
                members
                    .iter()
                    .find(|m| m["nodeId"].as_str() == Some(&owner_node_id))
                    .and_then(|m| m["displayName"].as_str())
                    .map(|n| {
                        n.to_lowercase()
                            .replace(' ', "-")
                            .chars()
                            .filter(|c| c.is_alphanumeric() || *c == '-')
                            .take(20)
                            .collect::<String>()
                    })
            })
            .unwrap_or_default()
    } else {
        String::new()
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

    // Try to clone from Gitea (gets all owner's files directly)
    let mut cloned = false;
    if let (Some(gitea_url), Some(_gitea_token), Some(gitea_user)) =
        (&cfg.gitea_url, &cfg.gitea_token, &cfg.gitea_user)
    {
        // Use owner's Gitea username for repo path, fallback to slug
        let repo_owner = if !owner_gitea_user.is_empty() {
            &owner_gitea_user
        } else {
            gitea_user
        };
        let repo_name = if !repo_name.is_empty() {
            &repo_name
        } else {
            &slug
        };
        let remote_url = format!(
            "http://{host}/{repo_owner}/{repo_name}.git",
            host = gitea_url
                .trim_start_matches("http://")
                .trim_start_matches("https://"),
            repo_owner = repo_owner,
            repo_name = repo_name,
        );
        match crate::sync_engine::git_clone(&remote_url, &project_dir) {
            Ok(()) => {
                println!("  Cloned project from Gitea");
                cloned = true;
            }
            Err(e) => {
                eprintln!("  Clone failed: {} (initializing fresh)", e);
            }
        }
    }

    // If clone failed, init fresh git repo
    if !cloned {
        if let Err(e) = crate::sync_engine::git_init(&project_dir) {
            eprintln!("  git init failed: {}", e);
        }
    }

    // Init .bridges/ and .shared/ (won't overwrite files from clone)
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

/// Auto-sync .shared/ before communication commands. Uses git push/pull to Gitea.
fn auto_sync(_project_id: &str) {
    // Sync is now manual-only via `bridges sync`.
    // Auto-sync before ask/debate caused git conflicts and slowed down messaging.
}

/// Ask another agent a question — auto-syncs, sends E2E encrypted, waits for response.
pub fn cmd_ask(node_id: &str, question: &str, project_id: Option<&str>, new_session: bool) {
    ensure_daemon();

    let pid = project_id.unwrap_or("");
    auto_sync(pid);

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

/// Start a debate — auto-syncs, sends to all members, collects responses.
pub fn cmd_debate(topic: &str, project_id: &str, new_session: bool) {
    ensure_daemon();
    auto_sync(project_id);
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

// ── Contact commands ──

pub fn cmd_contact_add(node_id: &str, name: Option<&str>) {
    let cfg = ClientConfig::load_or_exit();
    let client = authed_client(&cfg);
    let body = serde_json::json!({
        "nodeId": node_id,
        "displayName": name,
    });
    let url = format!("{}/v1/contacts", cfg.coordination);
    let resp = send_or_exit(&client, &url, Some(&body), "POST");
    if !resp.status().is_success() {
        eprintln!("Failed to add contact: HTTP {}", resp.status());
        std::process::exit(1);
    }
    let val: serde_json::Value = parse_json_or_exit(resp);
    if val["ok"].as_bool() == Some(true) {
        let display = name.unwrap_or(node_id);
        println!("Added contact: {} ({})", display, node_id);
    } else {
        eprintln!(
            "{}",
            val["message"].as_str().unwrap_or("Failed to add contact")
        );
        std::process::exit(1);
    }
}

pub fn cmd_contact_list() {
    let cfg = ClientConfig::load_or_exit();
    let client = authed_client(&cfg);
    let url = format!("{}/v1/contacts", cfg.coordination);
    let resp = send_or_exit(&client, &url, None, "GET");
    if !resp.status().is_success() {
        eprintln!("Failed to list contacts: HTTP {}", resp.status());
        std::process::exit(1);
    }
    let contacts: Vec<serde_json::Value> = parse_json_or_exit(resp);
    if contacts.is_empty() {
        println!("No contacts yet. Add one with: bridges contact add <node_id>");
        return;
    }
    println!("Contacts:");
    for c in &contacts {
        let nid = c["nodeId"].as_str().unwrap_or("?");
        let name = c["displayName"]
            .as_str()
            .or(c["registeredName"].as_str())
            .unwrap_or("");
        if name.is_empty() {
            println!("  {}", nid);
        } else {
            println!("  {} ({})", nid, name);
        }
    }
}

pub fn cmd_contact_remove(node_id: &str) {
    let cfg = ClientConfig::load_or_exit();
    let client = authed_client(&cfg);
    let url = format!("{}/v1/contacts/{}", cfg.coordination, node_id);
    let resp = match client.delete(&url).send() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Request failed: {}", e);
            std::process::exit(1);
        }
    };
    if resp.status().is_success() {
        println!("Removed contact: {}", node_id);
    } else if resp.status().as_u16() == 404 {
        eprintln!("Contact {} not found", node_id);
    } else {
        eprintln!("Failed: HTTP {}", resp.status());
    }
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

// ── Gitea helpers ──

/// Resolve Gitea API coordinates from a project ID.
/// Returns (gitea_url, token, owner, repo_name).
fn resolve_gitea_repo(project_id: &str) -> (String, String, String, String) {
    require_project_id(project_id);
    let cfg = ClientConfig::load_or_exit();
    let gitea_url = cfg.gitea_url.unwrap_or_else(|| {
        eprintln!("Gitea not configured. Run bridges setup first.");
        std::process::exit(1);
    });
    let gitea_token = cfg.gitea_token.unwrap_or_else(|| {
        eprintln!("Gitea token not found. Run bridges setup first.");
        std::process::exit(1);
    });

    let project_dir = resolve_project_dir(project_id).unwrap_or_else(|| {
        eprintln!("Project {} not found locally", project_id);
        std::process::exit(1);
    });

    let (owner, repo) =
        crate::sync_engine::git_get_remote_owner_repo(&project_dir).unwrap_or_else(|_| {
            // Fallback: use gitea_user + project slug
            let user = cfg.gitea_user.unwrap_or_default();
            let conn = crate::db::open_db();
            crate::db::init_db(&conn);
            let slug = crate::queries::get_project_by_id(&conn, project_id)
                .map(|p| p.slug)
                .unwrap_or_else(|| project_id.replace("proj_", ""));
            (user, slug)
        });

    (gitea_url, gitea_token, owner, repo)
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

// ── Issue commands ──

pub fn cmd_issue_create(title: &str, project_id: &str, body: Option<&str>, assign: Option<&str>) {
    let (url, token, owner, repo) = resolve_gitea_repo(project_id);
    let assignees: Vec<&str> = assign
        .map(|a| {
            a.split(',')
                .map(|s| s.trim().trim_start_matches('@'))
                .collect()
        })
        .unwrap_or_default();
    match crate::sync_engine::gitea_create_issue(
        &url,
        &token,
        &owner,
        &repo,
        title,
        body.unwrap_or(""),
        &assignees,
    ) {
        Ok(num) => println!("Created issue #{}: {}", num, title),
        Err(e) => eprintln!("Failed to create issue: {}", e),
    }
}

pub fn cmd_issue_list(project_id: &str) {
    let (url, token, owner, repo) = resolve_gitea_repo(project_id);
    match crate::sync_engine::gitea_list_issues(&url, &token, &owner, &repo) {
        Ok(issues) => {
            if issues.is_empty() {
                println!("No open issues");
                return;
            }
            for issue in &issues {
                let num = issue["number"].as_u64().unwrap_or(0);
                let title = issue["title"].as_str().unwrap_or("?");
                let state = issue["state"].as_str().unwrap_or("?");
                let assignees: Vec<&str> = issue["assignees"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|u| u["login"].as_str()).collect())
                    .unwrap_or_default();
                let assign_str = if assignees.is_empty() {
                    String::new()
                } else {
                    format!(" → {}", assignees.join(", "))
                };
                println!("  #{} [{}] {}{}", num, state, title, assign_str);
            }
        }
        Err(e) => eprintln!("Failed to list issues: {}", e),
    }
}

pub fn cmd_issue_show(number: u64, project_id: &str) {
    let (url, token, owner, repo) = resolve_gitea_repo(project_id);
    match crate::sync_engine::gitea_get_issue(&url, &token, &owner, &repo, number) {
        Ok(issue) => {
            let title = issue["title"].as_str().unwrap_or("?");
            let state = issue["state"].as_str().unwrap_or("?");
            let body = issue["body"].as_str().unwrap_or("");
            let user = issue["user"]["login"].as_str().unwrap_or("?");
            println!("#{} [{}] {}", number, state, title);
            println!("  by {}", user);
            if !body.is_empty() {
                println!("\n{}", body);
            }
        }
        Err(e) => eprintln!("Failed to get issue: {}", e),
    }
}

pub fn cmd_issue_comment(number: u64, text: &str, project_id: &str) {
    let (url, token, owner, repo) = resolve_gitea_repo(project_id);
    match crate::sync_engine::gitea_comment_issue(&url, &token, &owner, &repo, number, text) {
        Ok(()) => println!("Comment added to issue #{}", number),
        Err(e) => eprintln!("Failed to comment: {}", e),
    }
}

pub fn cmd_issue_close(number: u64, project_id: &str) {
    let (url, token, owner, repo) = resolve_gitea_repo(project_id);
    match crate::sync_engine::gitea_close_issue(&url, &token, &owner, &repo, number) {
        Ok(()) => println!("Issue #{} closed", number),
        Err(e) => eprintln!("Failed to close issue: {}", e),
    }
}

// ── Milestone commands ──

pub fn cmd_milestone_create(title: &str, project_id: &str, due: Option<&str>) {
    let (url, token, owner, repo) = resolve_gitea_repo(project_id);
    match crate::sync_engine::gitea_create_milestone(&url, &token, &owner, &repo, title, due) {
        Ok(id) => {
            let due_str = due.map(|d| format!(" (due {})", d)).unwrap_or_default();
            println!("Created milestone #{}: {}{}", id, title, due_str);
        }
        Err(e) => eprintln!("Failed to create milestone: {}", e),
    }
}

pub fn cmd_milestone_list(project_id: &str) {
    let (url, token, owner, repo) = resolve_gitea_repo(project_id);
    match crate::sync_engine::gitea_list_milestones(&url, &token, &owner, &repo) {
        Ok(milestones) => {
            if milestones.is_empty() {
                println!("No milestones");
                return;
            }
            for m in &milestones {
                let title = m["title"].as_str().unwrap_or("?");
                let state = m["state"].as_str().unwrap_or("?");
                let due = m["due_on"].as_str().unwrap_or("no due date");
                let open = m["open_issues"].as_u64().unwrap_or(0);
                let closed = m["closed_issues"].as_u64().unwrap_or(0);
                println!(
                    "  {} [{}] {}/{} issues — due {}",
                    title,
                    state,
                    closed,
                    open + closed,
                    due
                );
            }
        }
        Err(e) => eprintln!("Failed to list milestones: {}", e),
    }
}

// ── PR commands ──

pub fn cmd_pr_create(title: &str, project_id: &str) {
    let (url, token, owner, repo) = resolve_gitea_repo(project_id);
    let project_dir = resolve_project_dir(project_id).unwrap_or_else(|| {
        eprintln!("Project not found locally");
        std::process::exit(1);
    });
    let branch = crate::sync_engine::git_current_branch(&project_dir);
    if branch == "main" {
        eprintln!("Cannot create PR from main. Create a feature branch first.");
        std::process::exit(1);
    }
    // Push the branch first
    crate::sync_engine::git_push(&project_dir, &branch).ok();
    match crate::sync_engine::gitea_create_pr(&url, &token, &owner, &repo, title, &branch, "main") {
        Ok(num) => println!("Created PR #{}: {} ({} → main)", num, title, branch),
        Err(e) => eprintln!("Failed to create PR: {}", e),
    }
}

pub fn cmd_pr_list(project_id: &str) {
    let (url, token, owner, repo) = resolve_gitea_repo(project_id);
    match crate::sync_engine::gitea_list_prs(&url, &token, &owner, &repo) {
        Ok(prs) => {
            if prs.is_empty() {
                println!("No open pull requests");
                return;
            }
            for pr in &prs {
                let num = pr["number"].as_u64().unwrap_or(0);
                let title = pr["title"].as_str().unwrap_or("?");
                let head = pr["head"]["label"].as_str().unwrap_or("?");
                let base = pr["base"]["label"].as_str().unwrap_or("main");
                println!("  PR #{} {} → {} — {}", num, head, base, title);
            }
        }
        Err(e) => eprintln!("Failed to list PRs: {}", e),
    }
}
