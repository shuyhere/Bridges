//! Gitea setup — download, configure, theme, and start Gitea as part of `bridges serve`.
//!
//! The server operator runs `bridges serve` and Gitea is handled automatically.
//! Users never interact with Gitea directly — their agents use the API.

use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::serve::GiteaConfig;

/// Gitea version to download.
const GITEA_VERSION: &str = "1.22.6";

/// LuGit theme release URL (v0.2.2 for Gitea 1.22.x).
const LUGIT_THEME_URL: &str = "https://github.com/lucas-labs/gitea-lugit-theme/releases/download/v0.2.2/gitea-lugit-theme.tar.gz";

/// Root directory for Gitea installation.
fn gitea_root() -> PathBuf {
    let base = directories::BaseDirs::new().expect("cannot find home dir");
    base.home_dir().join(".bridges").join("gitea")
}

/// Path to the Gitea binary.
fn gitea_binary() -> PathBuf {
    gitea_root().join("gitea")
}

/// Path to Gitea custom directory.
fn gitea_custom() -> PathBuf {
    gitea_root().join("custom")
}

/// Path to app.ini.
fn gitea_app_ini() -> PathBuf {
    gitea_custom().join("conf").join("app.ini")
}

/// Path to the admin config file.
fn admin_config_path() -> PathBuf {
    let base = directories::BaseDirs::new().expect("cannot find home dir");
    base.home_dir().join(".gitea-admin.json")
}

/// Ensure Gitea is fully set up and start it. Returns the child process and config.
pub fn ensure_and_start(port: u16) -> Result<(Option<Child>, GiteaConfig), String> {
    // 1. Check if Gitea is already running on this port
    if is_gitea_running(port) {
        println!("  Gitea already running on port {}", port);
        let config = load_or_create_admin_config(port)?;
        return Ok((None, config));
    }

    // 2. Find available port
    let actual_port = find_available_port(port);
    if actual_port != port {
        println!("  Port {} in use, using {}", port, actual_port);
    }

    // 3. Ensure binary exists
    ensure_gitea_binary()?;

    // 4. Ensure configuration
    ensure_gitea_config(actual_port)?;

    // 5. Install theme
    install_lugit_theme().ok(); // non-fatal if theme download fails

    // 6. Start Gitea
    let child = start_gitea(actual_port)?;

    // 7. Wait for Gitea to be ready
    wait_for_gitea(actual_port)?;

    // 8. Auto-setup admin account if first run
    let config = load_or_create_admin_config(actual_port)?;

    Ok((Some(child), config))
}

/// Check if Gitea is already running on a port.
fn is_gitea_running(port: u16) -> bool {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();
    client
        .get(format!("http://127.0.0.1:{}/api/v1/version", port))
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Find an available port, starting from preferred.
fn find_available_port(preferred: u16) -> u16 {
    for port in preferred..preferred + 10 {
        if is_gitea_running(port) {
            return port; // reuse existing Gitea
        }
        if TcpListener::bind(("0.0.0.0", port)).is_ok() {
            return port; // port is free
        }
    }
    eprintln!("Warning: no available port for Gitea near {}", preferred);
    preferred
}

/// Download Gitea binary if not present.
fn ensure_gitea_binary() -> Result<(), String> {
    let binary = gitea_binary();
    if binary.exists() {
        return Ok(());
    }

    let root = gitea_root();
    fs::create_dir_all(&root).map_err(|e| format!("create gitea dir: {}", e))?;

    // Detect platform
    let (os, arch) = detect_platform();
    let url = format!(
        "https://dl.gitea.com/gitea/{}/gitea-{}-{}-{}",
        GITEA_VERSION, GITEA_VERSION, os, arch
    );

    println!("  Downloading Gitea {} ({}-{})...", GITEA_VERSION, os, arch);

    let resp = reqwest::blocking::get(&url).map_err(|e| format!("download gitea: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("download gitea HTTP {}", resp.status()));
    }

    let bytes = resp
        .bytes()
        .map_err(|e| format!("read gitea binary: {}", e))?;
    fs::write(&binary, &bytes).map_err(|e| format!("write gitea binary: {}", e))?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).ok();
    }

    println!("  Gitea downloaded to {}", binary.display());
    Ok(())
}

/// Detect OS and arch for download URL.
fn detect_platform() -> (&'static str, &'static str) {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    };

    (os, arch)
}

/// Generate app.ini if not present.
fn ensure_gitea_config(port: u16) -> Result<(), String> {
    let ini_path = gitea_app_ini();
    if ini_path.exists() {
        return Ok(());
    }

    let root = gitea_root();
    let conf_dir = gitea_custom().join("conf");
    fs::create_dir_all(&conf_dir).map_err(|e| format!("create conf dir: {}", e))?;
    fs::create_dir_all(root.join("data")).ok();
    fs::create_dir_all(root.join("repositories")).ok();
    fs::create_dir_all(root.join("log")).ok();

    let ini_content = format!(
        r#"[server]
HTTP_PORT = {port}
ROOT_URL = http://0.0.0.0:{port}/
OFFLINE_MODE = true
LFS_START_SERVER = false

[database]
DB_TYPE = sqlite3
PATH = {db_path}

[repository]
ROOT = {repo_path}

[ui]
DEFAULT_THEME = gitea
THEMES = gitea,arc-green

[service]
DISABLE_REGISTRATION = true
REQUIRE_SIGNIN_VIEW = false

[security]
INSTALL_LOCK = true
INTERNAL_TOKEN = {internal_token}

[log]
ROOT_PATH = {log_path}
MODE = file
LEVEL = warn
"#,
        port = port,
        db_path = root.join("gitea.db").to_string_lossy(),
        repo_path = root.join("repositories").to_string_lossy(),
        log_path = root.join("log").to_string_lossy(),
        internal_token = generate_internal_token(),
    );

    fs::write(&ini_path, ini_content).map_err(|e| format!("write app.ini: {}", e))?;
    println!("  Gitea config written to {}", ini_path.display());
    Ok(())
}

/// Generate a random internal token for Gitea.
fn generate_internal_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Download and install the LuGit theme.
fn install_lugit_theme() -> Result<(), String> {
    let css_dir = gitea_custom().join("public").join("assets").join("css");
    // Check if theme is already installed (we rename files to lugit-* prefix)
    if css_dir.join("theme-lugit-dark.css").exists() {
        return Ok(());
    }

    println!("  Downloading LuGit theme...");

    let resp =
        reqwest::blocking::get(LUGIT_THEME_URL).map_err(|e| format!("download theme: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("download theme HTTP {}", resp.status()));
    }

    let bytes = resp.bytes().map_err(|e| format!("read theme: {}", e))?;

    // Write tar.gz to temp file and extract
    let temp_path = gitea_root().join("lugit-theme.tar.gz");
    fs::write(&temp_path, &bytes).map_err(|e| format!("write theme: {}", e))?;

    let custom = gitea_custom();
    fs::create_dir_all(&custom).ok();

    // Extract using tar command
    let output = Command::new("tar")
        .args([
            "xzf",
            &temp_path.to_string_lossy(),
            "-C",
            &custom.to_string_lossy(),
        ])
        .output()
        .map_err(|e| format!("extract theme: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("extract theme failed: {}", stderr));
    }

    // The tar extracts theme-dark.css, theme-light.css, theme-auto.css
    // Rename to theme-lugit-* to avoid conflicting with Gitea built-in themes
    fs::create_dir_all(&css_dir).ok();
    for name in &["dark", "light", "auto"] {
        let src = css_dir.join(format!("theme-{}.css", name));
        let dst = css_dir.join(format!("theme-lugit-{}.css", name));
        if src.exists() && !dst.exists() {
            fs::rename(&src, &dst).ok();
        }
    }

    // Update app.ini to include lugit themes
    let ini_path = gitea_app_ini();
    if ini_path.exists() {
        let mut content = fs::read_to_string(&ini_path).unwrap_or_default();
        content = content.replace(
            "THEMES = gitea,arc-green",
            "THEMES = lugit-auto,lugit-dark,lugit-light,gitea,arc-green",
        );
        content = content.replace("DEFAULT_THEME = gitea", "DEFAULT_THEME = lugit-auto");
        fs::write(&ini_path, content).ok();
    }

    fs::remove_file(&temp_path).ok();
    println!("  LuGit theme installed");
    Ok(())
}

/// Start Gitea as a child process.
fn start_gitea(port: u16) -> Result<Child, String> {
    let binary = gitea_binary();
    let custom = gitea_custom();
    let root = gitea_root();

    println!("  Starting Gitea on port {}...", port);

    let child = Command::new(&binary)
        .args(["web", "--port", &port.to_string()])
        .env("GITEA_CUSTOM", custom.to_string_lossy().as_ref())
        .env("GITEA_WORK_DIR", root.to_string_lossy().as_ref())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("start gitea: {}", e))?;

    Ok(child)
}

/// Wait for Gitea to respond (up to 15 seconds).
fn wait_for_gitea(port: u16) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();
    let url = format!("http://127.0.0.1:{}/api/v1/version", port);

    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if client
            .get(&url)
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            println!("  Gitea ready on port {}", port);
            return Ok(());
        }
    }
    Err(format!(
        "Gitea did not start within 15 seconds on port {}",
        port
    ))
}

/// Load existing admin config or create admin account on first run.
fn load_or_create_admin_config(port: u16) -> Result<GiteaConfig, String> {
    let config_path = admin_config_path();

    // Try loading existing config
    if config_path.exists() {
        if let Ok(data) = fs::read_to_string(&config_path) {
            if let Ok(config) = serde_json::from_str::<GiteaConfig>(&data) {
                return Ok(config);
            }
        }
    }

    // First run — create admin account
    println!("  Creating Gitea admin account...");
    let gitea_url = format!("http://127.0.0.1:{}", port);
    let admin_user = "bridges-admin";
    let admin_password = format!("bridges_{}", uuid::Uuid::new_v4());
    let admin_email = "admin@bridges.local";

    // Create admin user via CLI (more reliable than API on first run)
    let binary = gitea_binary();
    let custom = gitea_custom();
    let root = gitea_root();

    let output = Command::new(&binary)
        .args([
            "admin",
            "user",
            "create",
            "--username",
            admin_user,
            "--password",
            &admin_password,
            "--email",
            admin_email,
            "--admin",
        ])
        .env("GITEA_CUSTOM", custom.to_string_lossy().as_ref())
        .env("GITEA_WORK_DIR", root.to_string_lossy().as_ref())
        .output()
        .map_err(|e| format!("create admin: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Ignore "user already exists" error
        if !stderr.contains("already exists") {
            return Err(format!("create admin failed: {}", stderr.trim()));
        }
    }

    // Generate admin API token via API
    let client = reqwest::blocking::Client::new();
    let token_body = serde_json::json!({
        "name": format!("bridges-admin-{}", chrono::Utc::now().timestamp()),
        "scopes": ["all"]
    });
    let token_resp = client
        .post(format!("{}/api/v1/users/{}/tokens", gitea_url, admin_user))
        .basic_auth(admin_user, Some(&admin_password))
        .json(&token_body)
        .send()
        .map_err(|e| format!("create admin token: {}", e))?;

    if !token_resp.status().is_success() {
        let text = token_resp.text().unwrap_or_default();
        return Err(format!("create admin token HTTP: {}", text));
    }

    let token_val: serde_json::Value = token_resp
        .json()
        .map_err(|e| format!("parse token: {}", e))?;
    let admin_token = token_val["sha1"]
        .as_str()
        .ok_or_else(|| "no sha1 in token response".to_string())?
        .to_string();

    let config = GiteaConfig {
        gitea_url,
        admin_user: admin_user.to_string(),
        admin_token,
        admin_password: Some(admin_password),
        external_url: None,
    };

    // Save config
    let json =
        serde_json::to_string_pretty(&config).map_err(|e| format!("serialize config: {}", e))?;
    fs::write(&config_path, &json).map_err(|e| format!("write admin config: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).ok();
    }

    println!(
        "  Gitea admin account created (config: {})",
        config_path.display()
    );
    Ok(config)
}
