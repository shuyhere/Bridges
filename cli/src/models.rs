use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub node_id: String,
    pub display_name: Option<String>,
    pub runtime: Option<String>,
    pub endpoint: Option<String>,
    pub public_key: String,
    pub owner_principal_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub node_id: String,
    pub display_name: Option<String>,
    pub runtime: Option<String>,
    pub endpoint: Option<String>,
    pub public_key: Option<String>,
    pub owner_name: Option<String>,
    pub trust_status: String,
    pub last_seen_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub project_id: String,
    pub slug: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub project_path: Option<String>,
    pub owner_principal_id: Option<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub project_id: String,
    pub peer_node_id: String,
    pub last_sync_at: Option<String>,
    pub last_version: Option<i64>,
}

/// Change record returned by sync
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub content: String,
    pub version: i64,
    pub changed_at: String,
    pub changed_by: String,
}
