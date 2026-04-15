use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectJson {
    pub project_id: String,
    pub slug: String,
    pub display_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchJson {
    pub poll_interval_secs: u64,
    pub peers: Vec<String>,
}

/// Initialize a .bridges workspace inside `project_path`.
/// Creates directory structure and default files. Never overwrites existing files.
pub fn init_workspace(project_path: &Path, slug: &str) {
    let bridges_dir = project_path.join(".bridges");
    let shared_dir = bridges_dir.join("shared");
    let artifacts_dir = shared_dir.join("artifacts");
    let peers_dir = bridges_dir.join("peers");

    // Create directories
    for dir in [&bridges_dir, &shared_dir, &artifacts_dir, &peers_dir] {
        fs::create_dir_all(dir).expect("failed to create workspace directory");
    }

    // project.json
    let project_json_path = bridges_dir.join("project.json");
    if !project_json_path.exists() {
        let project = ProjectJson {
            project_id: uuid::Uuid::new_v4().to_string(),
            slug: slug.to_string(),
            display_name: slug.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let json = serde_json::to_string_pretty(&project).unwrap();
        fs::write(&project_json_path, json).expect("failed to write project.json");
    }

    // watch.json
    let watch_path = bridges_dir.join("watch.json");
    if !watch_path.exists() {
        let watch = WatchJson {
            poll_interval_secs: 30,
            peers: vec![],
        };
        let json = serde_json::to_string_pretty(&watch).unwrap();
        fs::write(&watch_path, json).expect("failed to write watch.json");
    }

    // Shared markdown files
    write_if_missing(
        &shared_dir.join("PROJECT.md"),
        &format!("# {}\n\nProject overview goes here.\n", slug),
    );
    write_if_missing(
        &shared_dir.join("TODOS.md"),
        "# TODOs\n\n- [ ] First task\n",
    );
    write_if_missing(
        &shared_dir.join("DEBATES.md"),
        "# Debates\n\nOpen discussions go here.\n",
    );
    write_if_missing(
        &shared_dir.join("DECISIONS.md"),
        "# Decisions\n\nFinalized decisions go here.\n",
    );
    write_if_missing(
        &shared_dir.join("PROGRESS.md"),
        "# Progress\n\nOptional shared status updates.\n",
    );
    write_if_missing(
        &shared_dir.join("CHANGELOG.md"),
        "# Changelog\n\nProject-level changes and decisions. Do not store chat transcripts here.\n",
    );
}

fn write_if_missing(path: &Path, content: &str) {
    if !path.exists() {
        fs::write(path, content).unwrap_or_else(|e| {
            eprintln!("warning: could not write {}: {}", path.display(), e);
        });
    }
}

/// Read project.json from a workspace.
pub fn read_project_json(project_path: &Path) -> Option<ProjectJson> {
    let path = project_path.join(".bridges").join("project.json");
    let data = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Read watch.json from a workspace.
pub fn read_watch_json(project_path: &Path) -> Option<WatchJson> {
    let path = project_path.join(".bridges").join("watch.json");
    let data = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}
