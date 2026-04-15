use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

type LoginUserRow = (String, String, Option<String>, bool, String, String);

use super::auth::{AuthUser, SessionAuth};
use super::ServerState;

// ── Request / Response types ──

#[derive(Deserialize)]
pub struct SignupReq {
    pub email: String,
    pub password: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResp {
    #[serde(rename = "sessionToken")]
    pub session_token: String,
    pub user: UserResp,
}

#[derive(Serialize, Clone)]
pub struct UserResp {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub email: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "emailVerified")]
    pub email_verified: bool,
    pub plan: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct UpdateProfileReq {
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    pub email: Option<String>,
}

#[derive(Deserialize)]
pub struct ChangePasswordReq {
    #[serde(rename = "currentPassword")]
    pub current_password: String,
    #[serde(rename = "newPassword")]
    pub new_password: String,
}

#[derive(Deserialize)]
pub struct ForgotPasswordReq {
    pub email: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct ResetPasswordReq {
    pub token: String,
    #[serde(rename = "newPassword")]
    pub new_password: String,
}

#[derive(Serialize)]
pub struct MessageResp {
    pub ok: bool,
    pub message: String,
}

// ── Password hashing ──

fn hash_password(password: &str) -> Result<String, StatusCode> {
    use argon2::password_hash::{rand_core::OsRng, SaltString};
    use argon2::{Argon2, PasswordHasher};
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn verify_password(password: &str, hash: &str) -> bool {
    use argon2::password_hash::PasswordHash;
    use argon2::{Argon2, PasswordVerifier};
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

// ── Validation ──

fn validate_email(email: &str) -> Result<(), (StatusCode, String)> {
    if !email.contains('@') || email.len() < 5 || email.len() > 254 {
        return Err((StatusCode::BAD_REQUEST, "Invalid email address".to_string()));
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), (StatusCode, String)> {
    if password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Password must be at least 8 characters".to_string(),
        ));
    }
    if password.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Password must be at most 128 characters".to_string(),
        ));
    }
    Ok(())
}

// ── Routes ──

pub fn public_routes(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/v1/auth/signup", post(signup))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/forgot-password", post(forgot_password))
        .route("/v1/auth/reset-password", post(reset_password))
        .with_state(state)
}

pub fn protected_routes(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/v1/auth/logout", post(logout))
        .route("/v1/user/me", get(get_me).patch(update_me))
        .route("/v1/user/change-password", post(change_password))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            SessionAuth::middleware,
        ))
        .with_state(state)
}

// ── Handlers ──

async fn signup(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<SignupReq>,
) -> Result<Json<AuthResp>, (StatusCode, Json<MessageResp>)> {
    let email = req.email.trim().to_lowercase();
    validate_email(&email).map_err(|(s, m)| {
        (
            s,
            Json(MessageResp {
                ok: false,
                message: m,
            }),
        )
    })?;
    validate_password(&req.password).map_err(|(s, m)| {
        (
            s,
            Json(MessageResp {
                ok: false,
                message: m,
            }),
        )
    })?;

    let password_hash = hash_password(&req.password).map_err(|s| {
        (
            s,
            Json(MessageResp {
                ok: false,
                message: "Failed to hash password".to_string(),
            }),
        )
    })?;

    let user_id = format!("usr_{}", uuid::Uuid::new_v4());
    let now = chrono::Utc::now().to_rfc3339();
    let display_name = req
        .display_name
        .unwrap_or_else(|| email.split('@').next().unwrap_or("user").to_string());

    let db = state.db.lock().await;

    // Check if email already exists
    let exists: bool = db
        .query_row(
            "SELECT 1 FROM users WHERE email = ?1",
            rusqlite::params![email],
            |_| Ok(()),
        )
        .is_ok();
    if exists {
        return Err((
            StatusCode::CONFLICT,
            Json(MessageResp {
                ok: false,
                message: "Email already registered".to_string(),
            }),
        ));
    }

    db.execute(
        "INSERT INTO users (user_id, email, password_hash, display_name, email_verified, plan, created_at) \
         VALUES (?1, ?2, ?3, ?4, FALSE, 'free', ?5)",
        rusqlite::params![user_id, email, password_hash, display_name, now],
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MessageResp {
                ok: false,
                message: format!("Database error: {}", e),
            }),
        )
    })?;
    drop(db);

    let session_token = super::auth::create_session_token(&state.jwt_secret, &user_id, &email)
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MessageResp {
                    ok: false,
                    message: "Failed to create session".to_string(),
                }),
            )
        })?;

    Ok(Json(AuthResp {
        session_token,
        user: UserResp {
            user_id,
            email,
            display_name: Some(display_name),
            email_verified: false,
            plan: "free".to_string(),
            created_at: now,
        },
    }))
}

async fn login(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<LoginReq>,
) -> Result<Json<AuthResp>, (StatusCode, Json<MessageResp>)> {
    let email = req.email.trim().to_lowercase();

    let db = state.db.lock().await;
    let user: Result<LoginUserRow, _> = db.query_row(
        "SELECT user_id, password_hash, display_name, email_verified, plan, created_at \
         FROM users WHERE email = ?1",
        rusqlite::params![email],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, bool>(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        },
    );
    drop(db);

    let (user_id, password_hash, display_name, email_verified, plan, created_at) =
        user.map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(MessageResp {
                    ok: false,
                    message: "Invalid email or password".to_string(),
                }),
            )
        })?;

    if !verify_password(&req.password, &password_hash) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(MessageResp {
                ok: false,
                message: "Invalid email or password".to_string(),
            }),
        ));
    }

    let session_token = super::auth::create_session_token(&state.jwt_secret, &user_id, &email)
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MessageResp {
                    ok: false,
                    message: "Failed to create session".to_string(),
                }),
            )
        })?;

    Ok(Json(AuthResp {
        session_token,
        user: UserResp {
            user_id,
            email,
            display_name,
            email_verified,
            plan,
            created_at,
        },
    }))
}

async fn logout() -> Json<MessageResp> {
    // Stateless JWT — client just discards the token.
    // Could add token blocklist for extra security later.
    Json(MessageResp {
        ok: true,
        message: "Logged out".to_string(),
    })
}

async fn get_me(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<UserResp>, StatusCode> {
    let db = state.db.lock().await;
    db.query_row(
        "SELECT user_id, email, display_name, email_verified, plan, created_at \
         FROM users WHERE user_id = ?1",
        rusqlite::params![auth.user_id],
        |row| {
            Ok(UserResp {
                user_id: row.get(0)?,
                email: row.get(1)?,
                display_name: row.get(2)?,
                email_verified: row.get::<_, bool>(3)?,
                plan: row.get(4)?,
                created_at: row.get(5)?,
            })
        },
    )
    .map(Json)
    .map_err(|_| StatusCode::NOT_FOUND)
}

async fn update_me(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<UpdateProfileReq>,
) -> Result<Json<MessageResp>, (StatusCode, Json<MessageResp>)> {
    let db = state.db.lock().await;

    if let Some(ref name) = req.display_name {
        db.execute(
            "UPDATE users SET display_name = ?1 WHERE user_id = ?2",
            rusqlite::params![name, auth.user_id],
        )
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MessageResp {
                    ok: false,
                    message: "Update failed".to_string(),
                }),
            )
        })?;
    }

    if let Some(ref email) = req.email {
        let email = email.trim().to_lowercase();
        validate_email(&email).map_err(|(s, m)| {
            (
                s,
                Json(MessageResp {
                    ok: false,
                    message: m,
                }),
            )
        })?;
        db.execute(
            "UPDATE users SET email = ?1, email_verified = FALSE WHERE user_id = ?2",
            rusqlite::params![email, auth.user_id],
        )
        .map_err(|e| {
            let msg = if e.to_string().contains("UNIQUE") {
                "Email already in use"
            } else {
                "Update failed"
            };
            (
                StatusCode::CONFLICT,
                Json(MessageResp {
                    ok: false,
                    message: msg.to_string(),
                }),
            )
        })?;
    }

    Ok(Json(MessageResp {
        ok: true,
        message: "Profile updated".to_string(),
    }))
}

async fn change_password(
    State(state): State<Arc<ServerState>>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<ChangePasswordReq>,
) -> Result<Json<MessageResp>, (StatusCode, Json<MessageResp>)> {
    validate_password(&req.new_password).map_err(|(s, m)| {
        (
            s,
            Json(MessageResp {
                ok: false,
                message: m,
            }),
        )
    })?;

    let db = state.db.lock().await;
    let current_hash: String = db
        .query_row(
            "SELECT password_hash FROM users WHERE user_id = ?1",
            rusqlite::params![auth.user_id],
            |row| row.get(0),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(MessageResp {
                    ok: false,
                    message: "User not found".to_string(),
                }),
            )
        })?;

    if !verify_password(&req.current_password, &current_hash) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(MessageResp {
                ok: false,
                message: "Current password is incorrect".to_string(),
            }),
        ));
    }

    let new_hash = hash_password(&req.new_password).map_err(|s| {
        (
            s,
            Json(MessageResp {
                ok: false,
                message: "Failed to hash password".to_string(),
            }),
        )
    })?;

    db.execute(
        "UPDATE users SET password_hash = ?1 WHERE user_id = ?2",
        rusqlite::params![new_hash, auth.user_id],
    )
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MessageResp {
                ok: false,
                message: "Password update failed".to_string(),
            }),
        )
    })?;

    Ok(Json(MessageResp {
        ok: true,
        message: "Password changed".to_string(),
    }))
}

async fn forgot_password(
    State(_state): State<Arc<ServerState>>,
    Json(req): Json<ForgotPasswordReq>,
) -> Json<MessageResp> {
    let _email = req.email.trim().to_lowercase();
    // TODO: Generate reset token, store in DB, send email via lettre/SendGrid
    // For now, return success regardless (don't leak whether email exists)
    Json(MessageResp {
        ok: true,
        message: "If that email is registered, a reset link has been sent".to_string(),
    })
}

async fn reset_password(
    State(_state): State<Arc<ServerState>>,
    Json(_req): Json<ResetPasswordReq>,
) -> Json<MessageResp> {
    // TODO: Validate reset token, update password
    Json(MessageResp {
        ok: true,
        message: "Password reset (not yet implemented)".to_string(),
    })
}
