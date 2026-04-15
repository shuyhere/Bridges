use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use std::sync::Arc;

use super::auth::create_session_token;
use super::ServerState;

#[derive(Deserialize)]
pub struct OAuthCallback {
    code: String,
    #[allow(dead_code)]
    state: Option<String>,
}

pub fn routes(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/v1/auth/github", get(github_redirect))
        .route("/v1/auth/github/callback", get(github_callback))
        .route("/v1/auth/google", get(google_redirect))
        .route("/v1/auth/google/callback", get(google_callback))
        .with_state(state)
}

// ══════════════════════════════════════════════════════
//  GitHub OAuth
// ══════════════════════════════════════════════════════

async fn github_redirect(State(state): State<Arc<ServerState>>) -> Response {
    let client_id = match &state.oauth.github_client_id {
        Some(id) => id,
        None => {
            return (StatusCode::NOT_IMPLEMENTED, "GitHub OAuth not configured").into_response()
        }
    };
    let redirect_uri = format!("{}/v1/auth/github/callback", state.base_url);
    let url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope=user:email",
        client_id,
        urlencoding(redirect_uri.as_str()),
    );
    Redirect::temporary(&url).into_response()
}

async fn github_callback(
    State(state): State<Arc<ServerState>>,
    Query(params): Query<OAuthCallback>,
) -> Response {
    let (client_id, client_secret) = match (
        &state.oauth.github_client_id,
        &state.oauth.github_client_secret,
    ) {
        (Some(id), Some(secret)) => (id, secret),
        _ => return (StatusCode::NOT_IMPLEMENTED, "GitHub OAuth not configured").into_response(),
    };

    // Exchange code for access token
    let client = reqwest::Client::new();
    let token_resp = match client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id": client_id,
            "client_secret": client_secret,
            "code": params.code,
        }))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("GitHub token exchange failed: {}", e),
            )
                .into_response()
        }
    };

    let token_data: serde_json::Value = match token_resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("GitHub token parse failed: {}", e),
            )
                .into_response()
        }
    };

    let access_token = match token_data["access_token"].as_str() {
        Some(t) => t.to_string(),
        None => return (StatusCode::BAD_GATEWAY, "No access_token from GitHub").into_response(),
    };

    // Get user info
    let user_resp = match client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("User-Agent", "bridges")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("GitHub user fetch failed: {}", e),
            )
                .into_response()
        }
    };

    let user_data: serde_json::Value = match user_resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("GitHub user parse failed: {}", e),
            )
                .into_response()
        }
    };

    let github_id = user_data["id"].as_i64().unwrap_or(0).to_string();
    let name = user_data["name"]
        .as_str()
        .or(user_data["login"].as_str())
        .unwrap_or("User")
        .to_string();

    // Get primary email
    let emails_resp = client
        .get("https://api.github.com/user/emails")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("User-Agent", "bridges")
        .send()
        .await;

    let email = if let Ok(resp) = emails_resp {
        let emails: Vec<serde_json::Value> = resp.json().await.unwrap_or_default();
        emails
            .iter()
            .find(|e| e["primary"].as_bool() == Some(true))
            .or(emails.first())
            .and_then(|e| e["email"].as_str())
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    };

    if email.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "Could not retrieve email from GitHub",
        )
            .into_response();
    }

    // Upsert user
    match upsert_oauth_user(&state, &email, &name, Some(&github_id), None).await {
        Ok(session_token) => {
            // Redirect to configured client callback with token
            Redirect::temporary(&format!("/login?token={}", session_token)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("OAuth user creation failed: {}", e),
        )
            .into_response(),
    }
}

// ══════════════════════════════════════════════════════
//  Google OAuth
// ══════════════════════════════════════════════════════

async fn google_redirect(State(state): State<Arc<ServerState>>) -> Response {
    let client_id = match &state.oauth.google_client_id {
        Some(id) => id,
        None => {
            return (StatusCode::NOT_IMPLEMENTED, "Google OAuth not configured").into_response()
        }
    };
    let redirect_uri = format!("{}/v1/auth/google/callback", state.base_url);
    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope=openid%20email%20profile",
        client_id,
        urlencoding(redirect_uri.as_str()),
    );
    Redirect::temporary(&url).into_response()
}

async fn google_callback(
    State(state): State<Arc<ServerState>>,
    Query(params): Query<OAuthCallback>,
) -> Response {
    let (client_id, client_secret) = match (
        &state.oauth.google_client_id,
        &state.oauth.google_client_secret,
    ) {
        (Some(id), Some(secret)) => (id, secret),
        _ => return (StatusCode::NOT_IMPLEMENTED, "Google OAuth not configured").into_response(),
    };

    let redirect_uri = format!("{}/v1/auth/google/callback", state.base_url);

    // Exchange code for tokens
    let client = reqwest::Client::new();
    let token_resp = match client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", params.code.as_str()),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Google token exchange failed: {}", e),
            )
                .into_response()
        }
    };

    let token_data: serde_json::Value = match token_resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Google token parse failed: {}", e),
            )
                .into_response()
        }
    };

    let access_token = match token_data["access_token"].as_str() {
        Some(t) => t.to_string(),
        None => return (StatusCode::BAD_GATEWAY, "No access_token from Google").into_response(),
    };

    // Get user info
    let user_resp = match client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Google user fetch failed: {}", e),
            )
                .into_response()
        }
    };

    let user_data: serde_json::Value = match user_resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Google user parse failed: {}", e),
            )
                .into_response()
        }
    };

    let google_id = user_data["id"].as_str().unwrap_or("").to_string();
    let name = user_data["name"].as_str().unwrap_or("User").to_string();
    let email = user_data["email"].as_str().unwrap_or("").to_string();

    if email.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "Could not retrieve email from Google",
        )
            .into_response();
    }

    match upsert_oauth_user(&state, &email, &name, None, Some(&google_id)).await {
        Ok(session_token) => {
            Redirect::temporary(&format!("/login?token={}", session_token)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("OAuth user creation failed: {}", e),
        )
            .into_response(),
    }
}

// ══════════════════════════════════════════════════════
//  Shared: upsert OAuth user
// ══════════════════════════════════════════════════════

async fn upsert_oauth_user(
    state: &ServerState,
    email: &str,
    display_name: &str,
    github_id: Option<&str>,
    google_id: Option<&str>,
) -> Result<String, String> {
    let db = state.db.lock().await;
    let now = chrono::Utc::now().to_rfc3339();

    // Check if user exists by email
    let existing: Option<(String, String)> = db
        .query_row(
            "SELECT user_id, email FROM users WHERE email = ?1",
            rusqlite::params![email],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    let (user_id, user_email) = if let Some((uid, uemail)) = existing {
        // Update OAuth IDs
        if let Some(gid) = github_id {
            db.execute(
                "UPDATE users SET github_id = ?1 WHERE user_id = ?2",
                rusqlite::params![gid, uid],
            )
            .ok();
        }
        if let Some(gid) = google_id {
            db.execute(
                "UPDATE users SET google_id = ?1 WHERE user_id = ?2",
                rusqlite::params![gid, uid],
            )
            .ok();
        }
        (uid, uemail)
    } else {
        // Create new user (no password — OAuth only)
        let uid = format!("usr_{}", uuid::Uuid::new_v4());
        // Use a random unguessable password hash for OAuth users
        let placeholder_hash = format!("oauth_no_password_{}", uuid::Uuid::new_v4());
        db.execute(
            "INSERT INTO users (user_id, email, password_hash, display_name, email_verified, github_id, google_id, plan, created_at) \
             VALUES (?1, ?2, ?3, ?4, TRUE, ?5, ?6, 'free', ?7)",
            rusqlite::params![uid, email, placeholder_hash, display_name, github_id, google_id, now],
        )
        .map_err(|e| format!("create oauth user: {}", e))?;
        (uid, email.to_string())
    };

    drop(db);

    create_session_token(&state.jwt_secret, &user_id, &user_email)
        .map_err(|e| format!("create session: {}", e))
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}
