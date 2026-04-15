pub mod auth;
pub mod contacts;
pub mod endpoints;
pub mod invites;
pub mod keys;
pub mod projects;
pub mod relay;
pub mod skills;

use axum::Router;
use rusqlite::Connection;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

/// Gitea admin config loaded from gitea-admin.json.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GiteaConfig {
    pub gitea_url: String,
    pub admin_user: String,
    pub admin_token: String,
    #[serde(default)]
    pub admin_password: Option<String>,
    /// External URL for clients (if different from gitea_url which is localhost)
    #[serde(default)]
    pub external_url: Option<String>,
}

/// Shared state for all server routes.
pub struct ServerState {
    pub db: Mutex<Connection>,
    pub gitea: Option<GiteaConfig>,
}

/// Initialize the server database schema.
pub fn init_server_db(conn: &Connection) {
    conn.execute_batch(SERVER_SCHEMA)
        .expect("failed to init server schema");
    add_column_if_missing(conn, "registered_nodes", "gitea_user", "TEXT");
    add_column_if_missing(conn, "registered_nodes", "user_id", "TEXT");
    add_column_if_missing(conn, "server_projects", "gitea_owner", "TEXT");
    add_column_if_missing(conn, "server_projects", "gitea_repo", "TEXT");
}

fn add_column_if_missing(conn: &Connection, table: &str, column: &str, column_type: &str) {
    let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {column_type}");
    if let Err(err) = conn.execute(&sql, []) {
        let msg = err.to_string();
        if !msg.contains("duplicate column name") {
            panic!("failed to migrate {table}.{column}: {msg}");
        }
    }
}

/// Build the full axum router for `bridges serve`.
pub fn router(state: Arc<ServerState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .merge(auth::routes(state.clone()))
        .merge(keys::routes(state.clone()))
        .merge(endpoints::routes(state.clone()))
        .merge(projects::routes(state.clone()))
        .merge(skills::routes(state.clone()))
        .merge(relay::routes(state.clone()))
        .merge(contacts::routes(state.clone()))
        .route("/health", axum::routing::get(health))
        .layer(cors)
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "ok": true }))
}

/// Start the coordination server.
pub async fn run(port: u16, db_path: &str) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| format!("open db: {}", e))?;
    init_server_db(&conn);

    let gitea = load_gitea_config();
    if let Some(ref g) = gitea {
        println!(
            "Gitea integration: {} (admin: {})",
            g.gitea_url, g.admin_user
        );
    } else {
        println!("Gitea integration: disabled (no ~/.gitea-admin.json)");
    }

    let state = Arc::new(ServerState {
        db: Mutex::new(conn),
        gitea,
    });
    let app = router(state);
    let addr = format!("0.0.0.0:{}", port);
    println!("Bridges coordination server on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("bind: {}", e))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve: {}", e))
}

/// Load Gitea admin config from ~/.gitea-admin.json.
fn load_gitea_config() -> Option<GiteaConfig> {
    let home = std::env::var("HOME").ok()?;
    let path = std::path::Path::new(&home).join(".gitea-admin.json");
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

const SERVER_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    user_id         TEXT PRIMARY KEY,
    email           TEXT UNIQUE,
    display_name    TEXT,
    created_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS user_tokens (
    token_id        TEXT PRIMARY KEY,
    user_id         TEXT NOT NULL,
    token_hash      TEXT NOT NULL,
    name            TEXT NOT NULL,
    scopes          TEXT NOT NULL DEFAULT 'all',
    prefix          TEXT NOT NULL DEFAULT '',
    last_used_at    TEXT,
    expires_at      TEXT,
    created_at      TEXT NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(user_id)
);

CREATE INDEX IF NOT EXISTS idx_user_tokens_hash ON user_tokens(token_hash);
CREATE INDEX IF NOT EXISTS idx_user_tokens_user ON user_tokens(user_id);

CREATE TABLE IF NOT EXISTS registered_nodes (
    node_id         TEXT PRIMARY KEY,
    ed25519_pubkey  TEXT NOT NULL,
    x25519_pubkey   TEXT NOT NULL,
    display_name    TEXT,
    owner_name      TEXT,
    gitea_user      TEXT,
    user_id         TEXT,
    api_key_hash    TEXT NOT NULL,
    endpoint_hints  TEXT,
    created_at      TEXT NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(user_id)
);

CREATE INDEX IF NOT EXISTS idx_nodes_user ON registered_nodes(user_id);

CREATE TABLE IF NOT EXISTS server_projects (
    project_id      TEXT PRIMARY KEY,
    slug            TEXT UNIQUE NOT NULL,
    display_name    TEXT,
    description     TEXT,
    created_by      TEXT NOT NULL,
    gitea_owner     TEXT,
    gitea_repo      TEXT,
    created_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS server_members (
    project_id      TEXT NOT NULL,
    node_id         TEXT NOT NULL,
    agent_role      TEXT,
    joined_at       TEXT NOT NULL,
    PRIMARY KEY (project_id, node_id)
);

CREATE TABLE IF NOT EXISTS server_invites (
    invite_id       TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL,
    token_hash      TEXT NOT NULL,
    created_by      TEXT NOT NULL,
    max_uses        INTEGER,
    use_count       INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS server_skills (
    skill_id        TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL,
    node_id         TEXT NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT,
    created_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS user_contacts (
    user_id         TEXT NOT NULL,
    contact_node_id TEXT NOT NULL,
    display_name    TEXT,
    added_at        TEXT NOT NULL,
    PRIMARY KEY (user_id, contact_node_id)
);
"#;
