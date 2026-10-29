use crate::{api, auth, config::AuthMode, crypto, AppState};
use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::{net::SocketAddr, sync::Arc};

#[derive(Deserialize)]
pub struct Registration {
    username: String,
    password: String,
}

pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<Registration>,
) -> Response {
    if !registration_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let name = input.username.trim().to_ascii_lowercase();
    if let Err(message) = validate_registration(&name, &input.password) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let address = api::client_ip(&state, &headers, peer);
    let keys = [
        format!("registration:ip:{address}"),
        format!(
            "registration:account:{}",
            crypto::owner_id("registration", &name)
        ),
    ];
    let outcome = create_account(&state, &name, input.password, &keys, address).await;
    registration_response(outcome, address)
}

fn registration_response(
    outcome: Result<bool, (StatusCode, &'static str)>,
    address: std::net::IpAddr,
) -> Response {
    match outcome {
        Ok(true) => (StatusCode::CREATED, Json(json!({"ok":true}))).into_response(),
        Ok(false) => {
            tracing::warn!(%address, "registration rejected for unavailable username");
            error(StatusCode::CONFLICT, "That username is unavailable.")
        }
        Err((status, message)) => error(status, message),
    }
}

fn registration_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    state.config.auth == AuthMode::Builtin
        && state.config.allow_registration
        && auth::same_origin(headers, &state.config.origin)
}

fn validate_registration(name: &str, password: &str) -> Result<(), &'static str> {
    if !auth::valid_username(name) {
        return Err(
            "Use 1–64 letters, numbers, dots, underscores, hyphens, or @ for your username.",
        );
    }
    if !(12..=1024).contains(&password.len()) {
        return Err("Use a password of 12 to 1024 bytes.");
    }
    Ok(())
}

async fn create_account(
    state: &AppState,
    name: &str,
    password: String,
    keys: &[String],
    address: std::net::IpAddr,
) -> Result<bool, (StatusCode, &'static str)> {
    let Ok(permit) = Arc::clone(&state.password_limit).try_acquire_owned() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Account creation is busy. Try again shortly.",
        ));
    };
    if !state.auth.reserve(keys) {
        tracing::warn!(%address, "registration cooldown or capacity limit");
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "Too many attempts. Wait for the cooldown before trying again.",
        ));
    }
    let store = Arc::clone(&state.store);
    let name = name.to_owned();
    let created = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let hash = auth::hash_password(&password)?;
        store.create_user(&name, &hash)
    })
    .await;
    let Ok(Ok(created)) = created else {
        state.auth.cancel_attempt(keys);
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Account creation is temporarily unavailable.",
        ));
    };
    Ok(created)
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}
