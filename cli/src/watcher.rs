use std::path::Path;
use std::time::Duration;

use crate::db;
use crate::models::SyncState;
use crate::queries;
use crate::workspace;
use ed25519_dalek::SigningKey;

/// Start a polling watcher that syncs with peers on an interval.
/// Reads watch.json for configuration. Runs until interrupted.
pub async fn start_watching(
    project_path: &Path,
    node_id: &str,
    keypair: &SigningKey,
) -> Result<(), String> {
    let watch_config = workspace::read_watch_json(project_path)
        .ok_or_else(|| "no .bridges/watch.json found".to_string())?;

    let project_json = workspace::read_project_json(project_path)
        .ok_or_else(|| "no .bridges/project.json found".to_string())?;

    let project_id = project_json.project_id;
    let interval = Duration::from_secs(watch_config.poll_interval_secs.max(5));

    println!(
        "Watching project '{}' every {}s for {} peers",
        project_json.slug,
        interval.as_secs(),
        watch_config.peers.len()
    );

    loop {
        for peer_node_id in &watch_config.peers {
            let conn = db::open_db();
            let peer = queries::get_peer(&conn, peer_node_id);
            let endpoint = match peer.and_then(|p| p.endpoint) {
                Some(ep) if !ep.is_empty() => ep,
                _ => continue,
            };

            let since = queries::get_sync_state(&conn, &project_id, peer_node_id)
                .and_then(|s| s.last_sync_at)
                .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());

            match fetch_changes_http(&endpoint, &project_id, &since, node_id, keypair).await {
                Ok(changes) if !changes.is_empty() => {
                    let shared = project_path.join(".bridges").join("shared");
                    let changelog = shared.join("CHANGELOG.md");
                    let mut log_entries = String::new();
                    let mut max_version: i64 = 0;

                    for change in &changes {
                        let dest = shared.join(&change.path);
                        if let Some(parent) = dest.parent() {
                            std::fs::create_dir_all(parent).ok();
                        }
                        std::fs::write(&dest, &change.content).ok();

                        log_entries.push_str(&format!(
                            "- [{}] {} updated by {} (v{})\n",
                            change.changed_at, change.path, change.changed_by, change.version
                        ));
                        if change.version > max_version {
                            max_version = change.version;
                        }
                    }

                    if !log_entries.is_empty() {
                        let existing = std::fs::read_to_string(&changelog).unwrap_or_default();
                        let now = chrono::Utc::now().to_rfc3339();
                        let updated = format!("{}\n## Sync {}\n\n{}\n", existing, now, log_entries);
                        std::fs::write(&changelog, updated).ok();
                    }

                    let now = chrono::Utc::now().to_rfc3339();
                    queries::upsert_sync_state(
                        &conn,
                        &SyncState {
                            project_id: project_id.clone(),
                            peer_node_id: peer_node_id.clone(),
                            last_sync_at: Some(now),
                            last_version: Some(max_version),
                        },
                    );

                    println!("  pulled {} changes from {}", changes.len(), peer_node_id);
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("  watch error from {}: {}", peer_node_id, e);
                }
            }
        }

        tokio::time::sleep(interval).await;
    }
}

/// HTTP-based change fetching (shared with sync.rs pattern).
async fn fetch_changes_http(
    endpoint: &str,
    project_id: &str,
    since: &str,
    node_id: &str,
    keypair: &SigningKey,
) -> Result<Vec<crate::models::FileChange>, String> {
    use base64::Engine;
    let timestamp = chrono::Utc::now().to_rfc3339();
    let to_sign = format!("fetch:{}:{}", project_id, timestamp);
    let sig_bytes = crate::identity::sign(to_sign.as_bytes(), keypair);
    let sig_b64 = base64::engine::general_purpose::STANDARD.encode(&sig_bytes);

    let client = reqwest::Client::new();
    let url = format!("{}/v1/changes", endpoint);
    let resp = client
        .get(&url)
        .query(&[("project_id", project_id), ("since", since)])
        .header("X-Bridges-Node", node_id)
        .header("X-Bridges-Sig", &sig_b64)
        .header("X-Bridges-Project", project_id)
        .send()
        .await
        .map_err(|e| format!("fetch_changes: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    resp.json().await.map_err(|e| format!("parse: {}", e))
}
