use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Client config stored at ~/.bridges/config.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub coordination: String,
    #[serde(rename = "nodeId")]
    pub node_id: String,
    #[serde(rename = "apiKey")]
    pub api_key: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    pub owner: Option<String>,
    #[serde(rename = "giteaUrl", skip_serializing_if = "Option::is_none")]
    pub gitea_url: Option<String>,
    #[serde(rename = "giteaUser", skip_serializing_if = "Option::is_none")]
    pub gitea_user: Option<String>,
    #[serde(rename = "giteaToken", skip_serializing_if = "Option::is_none")]
    pub gitea_token: Option<String>,
    #[serde(rename = "giteaPassword", skip_serializing_if = "Option::is_none")]
    pub gitea_password: Option<String>,
}

fn config_path() -> PathBuf {
    let base = directories::BaseDirs::new().expect("cannot find home dir");
    base.home_dir().join(".bridges").join("config.json")
}

impl ClientConfig {
    /// Load from ~/.bridges/config.json. Returns None if missing.
    pub fn load() -> Option<Self> {
        let path = config_path();
        let data = fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Load or exit with helpful error.
    pub fn load_or_exit() -> Self {
        Self::load().unwrap_or_else(|| {
            eprintln!("Not registered. Run: bridges register --coordination <url>");
            std::process::exit(1);
        })
    }

    /// Save to ~/.bridges/config.json with restrictive permissions (0600).
    pub fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create .bridges dir");
            // Set directory to 0700 (owner only)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).ok();
            }
        }
        let json = serde_json::to_string_pretty(self).unwrap();
        fs::write(&path, &json).expect("write config.json");
        // Set file to 0600 (owner read/write only — contains API key)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).ok();
        }
    }
}
