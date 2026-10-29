use crate::{config::AuthMode, crypto, AppState};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderMap, Method, StatusCode},
    middleware::Next,
    response::Response,
};
use rand::rngs::OsRng;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Mutex,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;

const SESSION_LIFETIME: Duration = Duration::from_secs(12 * 60 * 60);
const CAPACITY: usize = 10_000;

#[derive(Clone, Debug)]
pub struct Identity {
    pub owner: String,
    pub name: String,
    pub session_token: Option<String>,
    pub password_hash: Option<String>,
}

struct Attempts {
    failures: u8,
    expires: Instant,
}
struct Session {
    identity: Identity,
    expires: Instant,
}

pub struct AuthState {
    attempts: Mutex<HashMap<String, Attempts>>,
    sessions: Mutex<HashMap<String, Session>>,
    cooldown: Duration,
}

impl AuthState {
    pub fn new(cooldown: Duration) -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            cooldown,
        }
    }
    pub fn blocked(&self, keys: &[String]) -> bool {
        let mut attempts = self.attempts.lock().expect("attempts lock");
        let now = Instant::now();
        attempts.retain(|_, entry| entry.expires > now);
        keys.iter()
            .any(|key| attempts.get(key).is_some_and(|entry| entry.failures >= 3))
    }

    pub fn reserve(&self, keys: &[String]) -> bool {
        let mut attempts = self.attempts.lock().expect("attempts lock");
        let now = Instant::now();
        attempts.retain(|_, entry| entry.expires > now);
        if attempts.len() + keys.len() > CAPACITY
            || keys
                .iter()
                .any(|key| attempts.get(key).is_some_and(|entry| entry.failures >= 3))
        {
            return false;
        }
        for key in keys {
            let entry = attempts.entry(key.clone()).or_insert(Attempts {
                failures: 0,
                expires: now + self.cooldown,
            });
            entry.failures = entry.failures.saturating_add(1);
            if entry.failures == 3 {
                entry.expires = now + self.cooldown;
            }
        }
        true
    }

    pub fn success(&self, keys: &[String]) {
        let mut attempts = self.attempts.lock().expect("attempts lock");
        for key in keys {
            attempts.remove(key);
        }
    }

    pub fn cancel_attempt(&self, keys: &[String]) {
        let mut attempts = self.attempts.lock().expect("attempts lock");
        for key in keys {
            if let Some(entry) = attempts.get_mut(key) {
                entry.failures = entry.failures.saturating_sub(1);
            }
        }
        attempts.retain(|_, entry| entry.failures > 0);
    }

    pub fn create_session(&self, identity: Identity) -> Option<String> {
        let mut sessions = self.sessions.lock().ok()?;
        sessions.retain(|_, session| session.expires > Instant::now());
        if sessions.len() >= CAPACITY {
            return None;
        }
        let token = crypto::random_token(32);
        sessions.insert(
            token.clone(),
            Session {
                identity,
                expires: Instant::now() + SESSION_LIFETIME,
            },
        );
        Some(token)
    }

    fn session(&self, token: &str) -> Option<Identity> {
        let mut sessions = self.sessions.lock().ok()?;
        sessions.retain(|_, session| session.expires > Instant::now());
        sessions.get(token).map(|s| s.identity.clone())
    }

    pub fn logout(&self, headers: &HeaderMap) {
        if let Some(token) = cookie(headers) {
            if let Ok(mut sessions) = self.sessions.lock() {
                sessions.remove(token);
            }
        }
    }
}

pub fn valid_username(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b'@'))
}

pub fn hash_password(password: &str) -> Result<String, String> {
    if password.len() < 12 || password.len() > 1024 {
        return Err("Use a password of 12 to 1024 bytes.".into());
    }
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|hash| hash.to_string())
        .map_err(|_| "Could not hash password.".into())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).ok().is_some_and(|parsed| {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

fn cookie(headers: &HeaderMap) -> Option<&str> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    cookies
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("speeching_session="))
}

pub fn cookie_header(token: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("speeching_session={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=43200{secure}")
}

fn identity(state: &AppState, headers: &HeaderMap, peer: SocketAddr) -> Option<Identity> {
    match state.config.auth {
        AuthMode::Off => Some(Identity {
            owner: crypto::owner_id("off", "guest"),
            name: "Guest".into(),
            session_token: None,
            password_hash: None,
        }),
        AuthMode::Builtin => {
            let token = cookie(headers)?;
            let mut user = state.auth.session(token)?;
            user.session_token = Some(token.to_owned());
            session_valid(state, &user).then_some(user)
        }
        AuthMode::Proxy => proxy_identity(state, headers, peer),
    }
}

fn proxy_identity(state: &AppState, headers: &HeaderMap, peer: SocketAddr) -> Option<Identity> {
    if !trusted_proxy(state, headers, peer) {
        return None;
    }
    let name = headers.get("x-speeching-user")?.to_str().ok()?;
    if !valid_username(name) {
        return None;
    }
    Some(Identity {
        owner: crypto::owner_id("proxy", name),
        name: name.to_owned(),
        session_token: None,
        password_hash: None,
    })
}

pub fn trusted_proxy(state: &AppState, headers: &HeaderMap, peer: SocketAddr) -> bool {
    if peer.ip() != state.config.trusted_proxy {
        return false;
    }
    let Some(expected) = state.config.proxy_secret.as_deref() else {
        return false;
    };
    let Some(provided) = headers.get("x-speeching-proxy-secret") else {
        return false;
    };
    bool::from(expected.as_bytes().ct_eq(provided.as_bytes()))
}

pub fn session_valid(state: &AppState, user: &Identity) -> bool {
    let Some(token) = &user.session_token else {
        return true;
    };
    if state.auth.session(token).is_none() {
        return false;
    }
    state.store.password_hash(&user.name).ok().flatten() == user.password_hash
}

pub fn same_origin(headers: &HeaderMap, expected: &str) -> bool {
    headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) == Some(expected)
}

pub async fn authorize(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let changing =
        request.method() != Method::GET || request.headers().contains_key(header::UPGRADE);
    if changing && !same_origin(request.headers(), &state.config.origin) {
        return Err(StatusCode::FORBIDDEN);
    }
    let user = identity(&state, request.headers(), peer).ok_or(StatusCode::UNAUTHORIZED)?;
    request.extensions_mut().insert(user);
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lockout_starts_after_three_failures_and_success_resets() {
        let state = AuthState::new(Duration::from_secs(300));
        let key = vec!["account:alice".to_owned()];
        assert!(state.reserve(&key));
        assert!(state.reserve(&key));
        assert!(!state.blocked(&key));
        assert!(state.reserve(&key));
        assert!(state.blocked(&key));
        assert!(!state.reserve(&key));
        state.cancel_attempt(&key);
        assert!(!state.blocked(&key));
        assert!(state.reserve(&key));
        state.success(&key);
        assert!(!state.blocked(&key));
    }
    #[test]
    fn passwords_origins_and_sessions_are_checked() {
        let hash = hash_password("long-enough-password").unwrap();
        assert!(verify_password("long-enough-password", &hash));
        assert!(!verify_password("incorrect-password", &hash));
        assert!(hash_password("short").is_err());
        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, "https://example.com".parse().unwrap());
        assert!(same_origin(&headers, "https://example.com"));
        assert!(!same_origin(&headers, "https://other.com"));
        let state = AuthState::new(Duration::from_secs(300));
        let token = state
            .create_session(Identity {
                owner: "a".into(),
                name: "a".into(),
                session_token: None,
                password_hash: None,
            })
            .unwrap();
        assert_eq!(state.session(&token).unwrap().owner, "a");
        headers.insert(
            header::COOKIE,
            format!("speeching_session={token}").parse().unwrap(),
        );
        state.logout(&headers);
        assert!(state.session(&token).is_none());
    }
    #[test]
    fn generated_bad_usernames_are_rejected() {
        for index in 0..1000 {
            assert!(!valid_username(&format!("name{index}\r\nInjected")));
        }
    }

    #[test]
    fn logout_and_password_reset_invalidate_existing_sessions() {
        let state = crate::tests::state();
        let hash = state.store.password_hash("alice").unwrap().unwrap();
        let mut user = Identity {
            owner: crypto::owner_id("builtin", "alice"),
            name: "alice".into(),
            session_token: None,
            password_hash: Some(hash),
        };
        let token = state.auth.create_session(user.clone()).unwrap();
        user.session_token = Some(token.clone());
        assert!(session_valid(&state, &user));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("speeching_session={token}").parse().unwrap(),
        );
        state.auth.logout(&headers);
        assert!(!session_valid(&state, &user));
        let token = state.auth.create_session(user.clone()).unwrap();
        user.session_token = Some(token);
        state
            .store
            .put_user("alice", &hash_password("another-password-123").unwrap())
            .unwrap();
        assert!(!session_valid(&state, &user));
    }
}
