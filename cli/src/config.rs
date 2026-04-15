use serde::{Deserialize, Serialize};
use std::fs;

/// Daemon-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    #[serde(default = "default_coord_url")]
    pub coordination_url: String,
    #[serde(default = "default_local_api_port")]
    pub local_api_port: u16,
    #[serde(default = "default_runtime")]
    pub runtime: String,
    #[serde(default)]
    pub runtime_endpoint: String,
    #[serde(default = "default_project_dir")]
    pub project_dir: String,
    #[serde(default = "default_stun_servers")]
    pub stun_servers: Vec<String>,
    #[serde(default = "default_derp_enabled")]
    pub derp_enabled: bool,
}

fn default_coord_url() -> String {
    std::env::var("BRIDGES_COORDINATION_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:17080".to_string())
}
fn default_local_api_port() -> u16 {
    7070
}
fn default_runtime() -> String {
    "claude-code".to_string()
}
fn default_project_dir() -> String {
    ".".to_string()
}
fn default_stun_servers() -> Vec<String> {
    vec!["64.233.186.127:19302".to_string()]
}
fn default_derp_enabled() -> bool {
    true
}

impl DaemonConfig {
    /// Load daemon config from ~/.bridges/daemon.json, with defaults.
    pub fn load() -> Self {
        let base = directories::BaseDirs::new();
        if let Some(base) = base {
            let path = base.home_dir().join(".bridges").join("daemon.json");
            if path.exists() {
                if let Ok(data) = fs::read_to_string(&path) {
                    if let Ok(cfg) = serde_json::from_str(&data) {
                        return cfg;
                    }
                }
            }
        }
        Self::default()
    }

    /// Get the API key. Reads from ~/.bridges/config.json first (where setup/register
    /// saves it), falls back to BRIDGES_API_KEY env var.
    pub fn api_key(&self) -> String {
        if let Some(cfg) = crate::client_config::ClientConfig::load() {
            if !cfg.api_key.is_empty() {
                return cfg.api_key;
            }
        }
        std::env::var("BRIDGES_API_KEY").unwrap_or_default()
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            coordination_url: default_coord_url(),
            local_api_port: default_local_api_port(),
            runtime: default_runtime(),
            runtime_endpoint: String::new(),
            project_dir: default_project_dir(),
            stun_servers: default_stun_servers(),
            derp_enabled: default_derp_enabled(),
        }
    }
}
