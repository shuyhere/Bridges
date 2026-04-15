mod client_config;
mod commands;
mod config;
mod connmgr;
mod conversation_memory;
mod coord_client;
mod crypto;
mod daemon;
mod db;
mod derp_client;
mod gitea_setup;
mod identity;
mod listener;
mod local_api;
mod mdns;
mod models;
mod noise;
mod queries;
mod serve;
mod service;
mod stun;
mod sync_engine;
mod transport;
mod watcher;
mod workspace;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "bridges",
    version,
    about = "Bridges — peer-to-peer Human Agent Network"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Bridges workspace in the current or given directory
    Init {
        #[arg(short, long)]
        slug: Option<String>,
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
    /// Start the Bridges daemon (encrypted P2P networking + listener)
    Daemon {
        #[arg(long, default_value = "true")]
        foreground: bool,
    },
    /// Manage the background daemon service
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// One-command setup: generate keys, register, configure, start daemon
    Setup {
        /// Coordination server URL
        #[arg(long, default_value = "http://127.0.0.1:17080")]
        coordination: String,
        /// API token from the web dashboard (skips registration)
        #[arg(long)]
        token: Option<String>,
        /// Runtime type (claude-code, codex, openclaw, generic)
        #[arg(long, default_value = "claude-code")]
        runtime: String,
        /// Runtime endpoint URL (for openclaw/generic)
        #[arg(long, default_value = "")]
        endpoint: String,
        /// Your display name (used for Gitea account and member list)
        #[arg(long)]
        name: Option<String>,
    },
    /// Sync .shared/ directory with all project peers
    Sync {
        /// Project ID (auto-resolves path)
        #[arg(short, long)]
        project: Option<String>,
        /// Override project path
        #[arg(long)]
        path: Option<PathBuf>,
        /// Approve syncing unmanaged paths after reviewing the generated proposal
        #[arg(long, default_value_t = false)]
        approve_unmanaged: bool,
    },
    /// Show node identity and project status
    Status,
    /// Watch for peer changes on a polling interval
    Watch {
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
    /// Ping a peer to test encrypted connectivity
    Ping {
        /// Node ID to ping (kd_xxx)
        node_id: String,
    },

    // ── Coordination commands ──
    /// Run the coordination server (+ Gitea for git sync & dashboard)
    Serve {
        #[arg(short, long, default_value = "17080")]
        port: u16,
        /// Gitea port for git hosting & dashboard
        #[arg(long, default_value = "3000")]
        gitea_port: u16,
        /// Path to server SQLite database
        #[arg(long, default_value = "./bridges-server.db")]
        db: String,
    },
    /// Register with a coordination server
    Register {
        #[arg(long)]
        coordination: String,
    },
    /// Create a project
    Create {
        /// Project name/slug
        name: String,
        #[arg(short, long)]
        description: Option<String>,
    },
    /// Generate an invite token for a project
    Invite {
        #[arg(short, long)]
        project: String,
    },
    /// Join a project using an invite token
    Join {
        /// Invite token
        invite_token: String,
        #[arg(short, long)]
        project: String,
    },
    /// List members of a project
    Members {
        #[arg(short, long)]
        project: String,
    },
    /// Ask another agent a question
    Ask {
        /// Target node ID
        node_id: String,
        /// Question text
        question: String,
        /// Project ID (optional — can chat without a project)
        #[arg(short, long, default_value = "")]
        project: String,
        /// Start a new conversation session instead of continuing the active one
        #[arg(long, default_value_t = false)]
        new_session: bool,
    },
    /// Start a debate with all project members
    Debate {
        /// Debate topic
        topic: String,
        #[arg(short, long)]
        project: String,
        /// Start a new conversation session with each peer instead of continuing the active one
        #[arg(long, default_value_t = false)]
        new_session: bool,
    },
    /// Inspect and manage conversation sessions with a peer
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// Manage your contacts
    Contact {
        #[command(subcommand)]
        action: ContactAction,
    },
    /// Broadcast a message to all project members
    Broadcast {
        /// Message text
        message: String,
        #[arg(short, long)]
        project: String,
    },
    /// Publish a file as an artifact to project members
    Publish {
        /// File path to publish
        file: String,
        #[arg(short, long)]
        project: String,
    },

    // ── Gitea project management ──
    /// Manage project issues
    Issue {
        #[command(subcommand)]
        action: IssueAction,
    },
    /// Manage project milestones
    Milestone {
        #[command(subcommand)]
        action: MilestoneAction,
    },
    /// Manage pull requests
    Pr {
        #[command(subcommand)]
        action: PrAction,
    },
}

#[derive(Subcommand)]
enum IssueAction {
    Create {
        title: String,
        #[arg(short, long)]
        project: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        assign: Option<String>,
    },
    List {
        #[arg(short, long)]
        project: String,
    },
    Show {
        number: u64,
        #[arg(short, long)]
        project: String,
    },
    Comment {
        number: u64,
        text: String,
        #[arg(short, long)]
        project: String,
    },
    Close {
        number: u64,
        #[arg(short, long)]
        project: String,
    },
}

#[derive(Subcommand)]
enum MilestoneAction {
    Create {
        title: String,
        #[arg(short, long)]
        project: String,
        #[arg(long)]
        due: Option<String>,
    },
    List {
        #[arg(short, long)]
        project: String,
    },
}

#[derive(Subcommand)]
enum PrAction {
    Create {
        title: String,
        #[arg(short, long)]
        project: String,
    },
    List {
        #[arg(short, long)]
        project: String,
    },
}

#[derive(Subcommand)]
enum ContactAction {
    /// Add a contact by node ID
    Add {
        /// Node ID to add (kd_xxx)
        node_id: String,
        /// Display name for this contact
        #[arg(long)]
        name: Option<String>,
    },
    /// List your contacts
    List,
    /// Remove a contact
    Remove {
        /// Node ID to remove
        node_id: String,
    },
}

#[derive(Subcommand)]
enum ServiceAction {
    Install,
    Uninstall,
    Start,
    Stop,
    Restart,
    Status,
}

#[derive(Subcommand)]
enum SessionAction {
    /// List sessions for a peer in a project
    List {
        #[arg(short, long)]
        project: String,
        #[arg(long)]
        peer: String,
    },
    /// Start a fresh session and make it active
    New {
        #[arg(short, long)]
        project: String,
        #[arg(long)]
        peer: String,
    },
    /// Switch the active session for a peer
    Use {
        session_id: String,
        #[arg(short, long)]
        project: String,
        #[arg(long)]
        peer: String,
    },
    /// Reset one session or all sessions for a peer
    Reset {
        #[arg(short, long)]
        project: String,
        #[arg(long)]
        peer: String,
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value_t = false)]
        all: bool,
    },
}

/// Entry point — most commands run without tokio (blocking HTTP).
/// Only daemon, serve, sync, watch, ping need the async runtime.
fn main() {
    let cli = Cli::parse();

    match cli.command {
        // ── Sync commands (no tokio, instant startup) ──
        Commands::Status => {
            let conn = db::open_db();
            db::init_db(&conn);
            drop(conn);
            let (_signing_key, verifying_key) = identity::load_or_create_keypair();
            let node_id = identity::derive_node_id(&verifying_key);
            cmd_status(&node_id, &verifying_key);
        }
        Commands::Init { slug, path } => {
            let conn = db::open_db();
            db::init_db(&conn);
            drop(conn);
            let (signing_key, verifying_key) = identity::load_or_create_keypair();
            let node_id = identity::derive_node_id(&verifying_key);
            cmd_init(&node_id, &signing_key, slug, path);
        }
        Commands::Register { coordination } => {
            commands::cmd_register(&coordination, None);
        }
        Commands::Setup {
            coordination,
            token,
            runtime,
            endpoint,
            name,
        } => {
            commands::cmd_setup(
                &coordination,
                token.as_deref(),
                &runtime,
                &endpoint,
                name.as_deref(),
            );
        }
        Commands::Create { name, description } => {
            commands::cmd_create(&name, description.as_deref());
        }
        Commands::Invite { project } => {
            commands::cmd_invite(&project);
        }
        Commands::Join {
            invite_token,
            project,
        } => {
            commands::cmd_join(&invite_token, &project);
        }
        Commands::Members { project } => {
            commands::cmd_members(&project);
        }
        Commands::Ask {
            node_id,
            question,
            project,
            new_session,
        } => {
            let proj = if project.is_empty() {
                None
            } else {
                Some(project.as_str())
            };
            commands::cmd_ask(&node_id, &question, proj, new_session);
        }
        Commands::Contact { action } => match action {
            ContactAction::Add { node_id, name } => {
                commands::cmd_contact_add(&node_id, name.as_deref());
            }
            ContactAction::List => {
                commands::cmd_contact_list();
            }
            ContactAction::Remove { node_id } => {
                commands::cmd_contact_remove(&node_id);
            }
        },
        Commands::Debate {
            topic,
            project,
            new_session,
        } => {
            commands::cmd_debate(&topic, &project, new_session);
        }
        Commands::Session { action } => match action {
            SessionAction::List { project, peer } => {
                commands::cmd_session_list(&project, &peer);
            }
            SessionAction::New { project, peer } => {
                commands::cmd_session_new(&project, &peer);
            }
            SessionAction::Use {
                session_id,
                project,
                peer,
            } => {
                commands::cmd_session_use(&project, &peer, &session_id);
            }
            SessionAction::Reset {
                project,
                peer,
                session,
                all,
            } => {
                commands::cmd_session_reset(&project, &peer, session.as_deref(), all);
            }
        },
        Commands::Broadcast { message, project } => {
            commands::cmd_broadcast(&message, &project);
        }
        Commands::Publish { file, project } => {
            commands::cmd_publish(&file, &project);
        }
        Commands::Service { action } => match action {
            ServiceAction::Install => match service::service_install() {
                Ok(msg) => println!("{}", msg),
                Err(e) => {
                    eprintln!("Service install failed: {}", e);
                    std::process::exit(1);
                }
            },
            ServiceAction::Uninstall => match service::service_uninstall() {
                Ok(msg) => println!("{}", msg),
                Err(e) => {
                    eprintln!("Service uninstall failed: {}", e);
                    std::process::exit(1);
                }
            },
            ServiceAction::Start => match service::service_start() {
                Ok(msg) => println!("{}", msg),
                Err(e) => {
                    eprintln!("Service start failed: {}", e);
                    std::process::exit(1);
                }
            },
            ServiceAction::Stop => match service::service_stop() {
                Ok(msg) => println!("{}", msg),
                Err(e) => {
                    eprintln!("Service stop failed: {}", e);
                    std::process::exit(1);
                }
            },
            ServiceAction::Restart => match service::service_restart() {
                Ok(msg) => println!("{}", msg),
                Err(e) => {
                    eprintln!("Service restart failed: {}", e);
                    std::process::exit(1);
                }
            },
            ServiceAction::Status => match service::service_status() {
                Ok(status) => println!("{}", status),
                Err(e) => {
                    eprintln!("Service status failed: {}", e);
                    std::process::exit(1);
                }
            },
        },
        Commands::Issue { action } => match action {
            IssueAction::Create {
                title,
                project,
                body,
                assign,
            } => {
                commands::cmd_issue_create(&title, &project, body.as_deref(), assign.as_deref());
            }
            IssueAction::List { project } => {
                commands::cmd_issue_list(&project);
            }
            IssueAction::Show { number, project } => {
                commands::cmd_issue_show(number, &project);
            }
            IssueAction::Comment {
                number,
                text,
                project,
            } => {
                commands::cmd_issue_comment(number, &text, &project);
            }
            IssueAction::Close { number, project } => {
                commands::cmd_issue_close(number, &project);
            }
        },
        Commands::Milestone { action } => match action {
            MilestoneAction::Create {
                title,
                project,
                due,
            } => {
                commands::cmd_milestone_create(&title, &project, due.as_deref());
            }
            MilestoneAction::List { project } => {
                commands::cmd_milestone_list(&project);
            }
        },
        Commands::Pr { action } => match action {
            PrAction::Create { title, project } => {
                commands::cmd_pr_create(&title, &project);
            }
            PrAction::List { project } => {
                commands::cmd_pr_list(&project);
            }
        },

        // ── Async commands (need tokio for long-running networking) ──
        Commands::Serve {
            port,
            gitea_port,
            db,
        } => {
            // Start Gitea first (blocking setup, then child process)
            let gitea_result = gitea_setup::ensure_and_start(gitea_port);
            let _gitea_child = match gitea_result {
                Ok((child, config)) => {
                    println!(
                        "  Gitea: {} (admin: {})",
                        config.gitea_url, config.admin_user
                    );
                    child
                }
                Err(e) => {
                    eprintln!("  Gitea setup failed: {} (continuing without)", e);
                    None
                }
            };

            // Then start bridges coordination server (async)
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
                .block_on(async {
                    if let Err(e) = serve::run(port, &db).await {
                        eprintln!("Server error: {}", e);
                    }
                });
        }
        Commands::Daemon { foreground } => {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
                .block_on(async {
                    if let Err(e) = daemon::run(foreground).await {
                        eprintln!("Daemon error: {}", e);
                    }
                });
        }
        Commands::Sync {
            project,
            path,
            approve_unmanaged,
        } => {
            let conn = db::open_db();
            db::init_db(&conn);

            let project_dir = if let Some(pid) = &project {
                queries::get_project_path(&conn, pid)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        eprintln!("Project {} not found locally", pid);
                        std::process::exit(1);
                    })
            } else if let Some(p) = path {
                p
            } else {
                eprintln!("Specify --project <ID> or --path <DIR>");
                std::process::exit(1);
            };
            drop(conn);

            let (_, verifying_key) = identity::load_or_create_keypair();
            let node_id = identity::derive_node_id(&verifying_key);

            sync_engine::init_shared(&project_dir);
            match sync_engine::sync_project(&project_dir, &node_id, approve_unmanaged) {
                Ok(result) => {
                    if result.pushed {
                        println!("Pushed local changes");
                    }
                    if result.pulled > 0 {
                        println!("Pulled {} changes", result.pulled);
                    }
                    if result.pulled == 0 && !result.pushed {
                        println!("Already up to date");
                    }
                    if !result.conflicts.is_empty() {
                        println!("CONFLICTS: {}", result.conflicts.join(", "));
                        println!("Resolve conflicts in .shared/ then sync again");
                    }
                    for warning in &result.warnings {
                        println!("SYNC WARNING: {}", warning);
                    }
                }
                Err(e) => eprintln!("Sync failed: {}", e),
            }
        }
        Commands::Watch { path } => {
            let conn = db::open_db();
            db::init_db(&conn);
            drop(conn);
            let (signing_key, verifying_key) = identity::load_or_create_keypair();
            let node_id = identity::derive_node_id(&verifying_key);
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
                .block_on(cmd_watch(&node_id, &signing_key, path));
        }
        Commands::Ping { node_id } => {
            let conn = db::open_db();
            db::init_db(&conn);
            drop(conn);
            let (signing_key, verifying_key) = identity::load_or_create_keypair();
            let my_node_id = identity::derive_node_id(&verifying_key);
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
                .block_on(cmd_ping(&my_node_id, &signing_key, &node_id));
        }
    }
}

fn cmd_init(
    node_id: &str,
    signing_key: &ed25519_dalek::SigningKey,
    slug: Option<String>,
    path: Option<PathBuf>,
) {
    let project_path = path.unwrap_or_else(|| std::env::current_dir().unwrap());
    let slug = slug.unwrap_or_else(|| {
        project_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string()
    });

    workspace::init_workspace(&project_path, &slug);

    if let Some(pj) = workspace::read_project_json(&project_path) {
        let conn = db::open_db();
        db::init_db(&conn);
        let verifying = signing_key.verifying_key();
        let pubkey_b58 = bs58::encode(verifying.as_bytes()).into_string();

        queries::insert_node(
            &conn,
            &models::Node {
                node_id: node_id.to_string(),
                display_name: Some(slug.clone()),
                runtime: Some("bridges-cli".to_string()),
                endpoint: None,
                public_key: pubkey_b58,
                owner_principal_id: None,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
        );

        queries::insert_project(
            &conn,
            &models::Project {
                project_id: pj.project_id.clone(),
                slug: pj.slug.clone(),
                display_name: Some(pj.display_name.clone()),
                description: None,
                project_path: Some(project_path.to_string_lossy().to_string()),
                owner_principal_id: None,
                status: "active".to_string(),
                created_at: pj.created_at.clone(),
            },
        );

        println!("Initialized Bridges workspace:");
        println!("  node:    {}", node_id);
        println!("  project: {} ({})", pj.slug, pj.project_id);
        println!("  path:    {}", project_path.display());
    }
}

fn cmd_status(node_id: &str, verifying_key: &ed25519_dalek::VerifyingKey) {
    let pubkey_b58 = bs58::encode(verifying_key.as_bytes()).into_string();
    let x25519_pub = crypto::ed25519_to_x25519_public(verifying_key.as_bytes())
        .expect("own Ed25519 key must be valid");

    println!("Bridges Node Status");
    println!("  node_id:      {}", node_id);
    println!("  ed25519_pub:  {}", pubkey_b58);
    println!("  x25519_pub:   {}", hex::encode(x25519_pub));

    // Show client config if available.
    if let Some(cfg) = client_config::ClientConfig::load() {
        println!("  coordination: {}", cfg.coordination);
        println!("  registered:   yes");
        if let (Some(url), Some(user)) = (cfg.gitea_url.as_deref(), cfg.gitea_user.as_deref()) {
            println!("  gitea:        yes");
            println!("    user:       {}", user);
            println!("    url:        {}", url);
        } else {
            println!("  gitea:        no");
            println!("    reason:     server did not provide Gitea credentials during setup");
        }
    } else {
        println!("  registered:   no");
    }

    let conn = db::open_db();
    db::init_db(&conn);

    let projects = queries::list_projects(&conn);
    if projects.is_empty() {
        println!("  projects:     (none)");
    } else {
        println!("  projects:");
        for p in &projects {
            let path = p.project_path.as_deref().unwrap_or("?");
            println!("    - {} [{}] {}", p.slug, p.status, p.project_id);
            println!("      path: {}", path);
        }
    }

    let peers = queries::list_peers(&conn);
    if peers.is_empty() {
        println!("  peers:        (none)");
    } else {
        println!("  peers:");
        for peer in &peers {
            let name = peer.display_name.as_deref().unwrap_or("?");
            println!("    - {} ({}) [{}]", peer.node_id, name, peer.trust_status);
        }
    }
}

async fn cmd_ping(node_id: &str, _signing_key: &ed25519_dalek::SigningKey, target: &str) {
    println!("Pinging {} ...", target);

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "peer_id": target,
        "message": format!("ping from {}", node_id),
    });

    match client
        .post("http://127.0.0.1:7070/send")
        .json(&body)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            println!("  ping sent via daemon (encrypted)");
        }
        Ok(resp) => {
            eprintln!("  daemon returned {}", resp.status());
        }
        Err(_) => {
            eprintln!("  daemon not running (start with: bridges daemon)");
        }
    }
}

async fn cmd_watch(node_id: &str, signing_key: &ed25519_dalek::SigningKey, path: Option<PathBuf>) {
    let project_path = path.unwrap_or_else(|| std::env::current_dir().unwrap());
    if let Err(e) = watcher::start_watching(&project_path, node_id, signing_key).await {
        eprintln!("Watch failed: {}", e);
    }
}
