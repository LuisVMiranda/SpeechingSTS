use crate::{
    auth::{self, Identity},
    config::AuthMode,
    crypto, AppState,
};
use axum::{
    extract::{ConnectInfo, Extension, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
fn error(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"error":message})))
}

#[derive(Deserialize)]
pub struct Login {
    username: String,
    password: String,
}

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> Response {
    if state.config.auth != AuthMode::Builtin || !auth::same_origin(&headers, &state.config.origin)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let name = input.username.trim().to_ascii_lowercase();
    let address = client_ip(&state, &headers, peer);
    let keys = vec![
        format!("ip:{address}"),
        format!("account:{}", crypto::owner_id("login", &name)),
    ];
    let (verified, credential_version) =
        match verify_login(&state, &name, input.password, &keys, address).await {
            Ok(result) => result,
            Err((status, message)) => return error(status, message).into_response(),
        };
    if !verified {
        tracing::warn!(%address, locked=state.auth.blocked(&keys), "failed sign-in");
        return error(StatusCode::UNAUTHORIZED, "Incorrect username or password.").into_response();
    }
    state.auth.success(&keys);
    let identity = Identity {
        owner: crypto::owner_id("builtin", &name),
        name,
        session_token: None,
        password_hash: Some(credential_version),
    };
    let Some(token) = state.auth.create_session(identity) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let cookie = auth::cookie_header(&token, state.config.origin.starts_with("https://"));
    ([(header::SET_COOKIE, cookie)], Json(json!({"ok":true}))).into_response()
}

async fn verify_login(
    state: &AppState,
    name: &str,
    password: String,
    keys: &[String],
    address: std::net::IpAddr,
) -> Result<(bool, String), (StatusCode, &'static str)> {
    let Ok(permit) = Arc::clone(&state.password_limit).try_acquire_owned() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Sign-in is busy. Try again shortly.",
        ));
    };
    let stored_hash = match state.store.password_hash(name) {
        Ok(value) => value,
        Err(_) => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "Sign-in is temporarily unavailable.",
            ))
        }
    };
    if !state.auth.reserve(keys) {
        tracing::warn!(%address, "sign-in cooldown or capacity limit");
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "Too many attempts. Wait for the cooldown before trying again.",
        ));
    }
    let account_exists = stored_hash.is_some();
    let hash = stored_hash.unwrap_or_else(|| state.dummy_hash.clone());
    let credential_version = hash.clone();
    let valid_name = auth::valid_username(name);
    let verification = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let matches = password.len() <= 1024 && auth::verify_password(&password, &hash);
        valid_name && account_exists && matches
    })
    .await;
    let verified = match verification {
        Ok(value) => value,
        Err(_) => {
            state.auth.cancel_attempt(keys);
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "Sign-in is temporarily unavailable.",
            ));
        }
    };
    Ok((verified, credential_version))
}

fn client_ip(state: &AppState, headers: &HeaderMap, peer: SocketAddr) -> std::net::IpAddr {
    if !state.config.trust_client_ip_header || !auth::trusted_proxy(state, headers, peer) {
        return peer.ip();
    }
    headers
        .get("x-speeching-client-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| peer.ip())
}

pub async fn mode(State(state): State<AppState>) -> Json<Value> {
    let auth = match state.config.auth {
        AuthMode::Builtin => "builtin",
        AuthMode::Proxy => "proxy",
        AuthMode::Off => "off",
    };
    Json(json!({"auth":auth}))
}

pub async fn me(State(state): State<AppState>, Extension(user): Extension<Identity>) -> ApiResult {
    let gemini = state
        .store
        .secret(&user.owner, "gemini")
        .map_err(internal)?
        .is_some();
    let openai = state
        .store
        .secret(&user.owner, "openai")
        .map_err(internal)?
        .is_some();
    Ok(Json(
        json!({"name":user.name,"keys":{"gemini":gemini,"openai":openai}}),
    ))
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    state.auth.logout(&headers);
    let cookie = format!(
        "speeching_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
        if state.config.origin.starts_with("https://") {
            "; Secure"
        } else {
            ""
        }
    );
    ([(header::SET_COOKIE, cookie)], Json(json!({"ok":true}))).into_response()
}

#[derive(Deserialize)]
pub struct Secret {
    key: String,
}
pub async fn put_key(
    State(state): State<AppState>,
    Extension(user): Extension<Identity>,
    Path(provider): Path<String>,
    Json(input): Json<Secret>,
) -> ApiResult {
    if !["gemini", "openai"].contains(&provider.as_str()) {
        return Err(error(StatusCode::BAD_REQUEST, "Unsupported provider."));
    }
    let encrypted = crypto::seal(&state.config.master_key, &user.owner, &provider, &input.key)
        .map_err(|message| error(StatusCode::BAD_REQUEST, &message))?;
    state
        .store
        .put_secret(&user.owner, &provider, &encrypted)
        .map_err(internal)?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Default, Deserialize)]
pub struct PageQuery {
    offset: Option<u32>,
    search: Option<String>,
    sort: Option<String>,
    revision: Option<i64>,
}

pub async fn chats(
    State(state): State<AppState>,
    Extension(user): Extension<Identity>,
    Query(page): Query<PageQuery>,
) -> ApiResult {
    let search = page.search.unwrap_or_default();
    if search.len() > 200 {
        return Err(error(StatusCode::BAD_REQUEST, "Search is too long."));
    }
    let (chats, more, revision) = state
        .store
        .list_page(
            &user.owner,
            page.offset.unwrap_or(0),
            &search,
            page.sort.as_deref().unwrap_or("newest"),
        )
        .map_err(internal)?;
    if page.offset.unwrap_or(0) > 0 && page.revision != Some(revision) {
        return Err(error(
            StatusCode::CONFLICT,
            "History changed. Refresh and try again.",
        ));
    }
    Ok(Json(json!({"chats":chats,"more":more,"revision":revision})))
}

pub async fn messages(
    State(state): State<AppState>,
    Extension(user): Extension<Identity>,
    Path(id): Path<String>,
    Query(page): Query<PageQuery>,
) -> ApiResult {
    let (messages, more, revision) = state
        .store
        .message_page(&user.owner, &id, page.offset.unwrap_or(0))
        .map_err(internal)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Conversation not found."))?;
    if page.offset.unwrap_or(0) > 0 && page.revision != Some(revision) {
        return Err(error(
            StatusCode::CONFLICT,
            "History changed. Refresh and try again.",
        ));
    }
    let chat = state
        .store
        .chat(&user.owner, &id)
        .map_err(internal)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Conversation not found."))?;
    Ok(Json(
        json!({"chat":chat,"messages":messages,"more":more,"revision":revision}),
    ))
}

#[derive(Deserialize)]
pub struct Rename {
    title: String,
}
pub async fn rename(
    State(state): State<AppState>,
    Extension(user): Extension<Identity>,
    Path(id): Path<String>,
    Json(input): Json<Rename>,
) -> ApiResult {
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > 100 {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Use a name between 1 and 100 characters.",
        ));
    }
    let updated = state
        .store
        .rename(&user.owner, &id, title)
        .map_err(internal)?;
    if !updated {
        return Err(error(StatusCode::NOT_FOUND, "Conversation not found."));
    }
    Ok(Json(json!({"ok":true})))
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(user): Extension<Identity>,
    Path(id): Path<String>,
) -> ApiResult {
    if !state.store.delete(&user.owner, &id).map_err(internal)? {
        return Err(error(StatusCode::NOT_FOUND, "Conversation not found."));
    }
    Ok(Json(json!({"ok":true})))
}

fn internal(_error: String) -> (StatusCode, Json<Value>) {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "The data store is unavailable.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forwarded_client_ip_is_accepted_only_from_a_configured_proxy() {
        let mut state = crate::tests::state();
        let mut config = (*state.config).clone();
        config.trust_client_ip_header = true;
        config.proxy_secret = Some("synthetic-proxy-secret-for-test-only".into());
        state.config = Arc::new(config);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-speeching-proxy-secret",
            "synthetic-proxy-secret-for-test-only".parse().unwrap(),
        );
        headers.insert("x-speeching-client-ip", "192.0.2.44".parse().unwrap());
        assert_eq!(
            client_ip(&state, &headers, "127.0.0.1:1234".parse().unwrap()).to_string(),
            "192.0.2.44"
        );
        assert_eq!(
            client_ip(&state, &headers, "127.0.0.2:1234".parse().unwrap()).to_string(),
            "127.0.0.2"
        );
        headers.insert("x-speeching-proxy-secret", "wrong-secret".parse().unwrap());
        assert_eq!(
            client_ip(&state, &headers, "127.0.0.1:1234".parse().unwrap()).to_string(),
            "127.0.0.1"
        );
        headers.insert(
            "x-speeching-proxy-secret",
            "synthetic-proxy-secret-for-test-only".parse().unwrap(),
        );
        headers.insert("x-speeching-client-ip", "invalid".parse().unwrap());
        assert_eq!(
            client_ip(&state, &headers, "127.0.0.1:1234".parse().unwrap()).to_string(),
            "127.0.0.1"
        );
    }
}
