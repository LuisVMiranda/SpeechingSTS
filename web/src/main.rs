mod api;
#[cfg(test)]
mod api_chat_tests;
mod auth;
mod config;
mod crypto;
mod live;
#[cfg(test)]
mod live_auth_tests;
#[cfg(test)]
mod live_tls_tests;
#[cfg(test)]
mod paging_tests;
mod protocol;
mod store;
#[cfg(test)]
mod tests;

use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderValue},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use config::Config;
use std::{env, path::Path, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    config: Arc<Config>,
    store: Arc<store::Store>,
    auth: Arc<auth::AuthState>,
    live_limit: Arc<Semaphore>,
    password_limit: Arc<Semaphore>,
    dummy_hash: String,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Speeching web: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [action] if action == "keygen" => {
            println!("{}", crypto::random_token(32));
            return Ok(());
        }
        [action, operation, name] if action == "user" && operation == "add" => {
            return add_user(name)
        }
        [] => {}
        _ => return Err("Usage: speeching-web [keygen | user add USERNAME]".into()),
    }
    serve().await
}

fn data_store(dir: &Path) -> Result<store::Store, String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Could not create the web data directory: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("Could not restrict web data directory permissions: {e}"))?;
    }
    store::Store::open(&dir.join("speeching-web.sqlite3"))
}

fn add_user(input: &str) -> Result<(), String> {
    let name = input.trim().to_ascii_lowercase();
    if !auth::valid_username(&name) {
        return Err(
            "Username must use 1–64 letters, digits, dots, underscores, hyphens, or @.".into(),
        );
    }
    let password = rpassword::prompt_password("Password (at least 12 characters): ")
        .map_err(|_| "Could not read password.")?;
    let repeat =
        rpassword::prompt_password("Repeat password: ").map_err(|_| "Could not read password.")?;
    if password != repeat {
        return Err("Passwords do not match.".into());
    }
    let hash = auth::hash_password(&password)?;
    data_store(&config::data_dir())?.put_user(&name, &hash)?;
    println!("Account saved: {name}");
    Ok(())
}

async fn serve() -> Result<(), String> {
    install_tls_provider()?;
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("speeching_web=info"))
        .init();
    let config = Config::load()?;
    let store = Arc::new(data_store(&config.data_dir)?);
    let state = AppState {
        live_limit: Arc::new(Semaphore::new(config.max_sessions)),
        password_limit: Arc::new(Semaphore::new(2)),
        auth: Arc::new(auth::AuthState::new(Duration::from_secs(
            config.cooldown_seconds,
        ))),
        dummy_hash: auth::hash_password("not-a-real-user-password")?,
        store,
        config: Arc::new(config),
    };
    let address = state.config.listen;
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|e| format!("Could not listen on {address}: {e}"))?;
    tracing::info!(%address, "Speeching web started");
    axum::serve(
        listener,
        app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await
    .map_err(|e| e.to_string())
}

fn install_tls_provider() -> Result<(), String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "Could not initialize the web service's TLS provider.".to_owned())
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

fn app(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/me", get(api::me))
        .route("/api/logout", post(api::logout))
        .route("/api/keys/{provider}", post(api::put_key))
        .route("/api/chats", get(api::chats))
        .route(
            "/api/chats/{id}",
            get(api::messages).patch(api::rename).delete(api::delete),
        )
        .route("/api/live", get(live::upgrade))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::authorize,
        ));
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(javascript))
        .route("/audio.js", get(audio_javascript))
        .route("/i18n.js", get(i18n_javascript))
        .route("/worklet.js", get(worklet))
        .route("/style.css", get(stylesheet))
        .route("/api/mode", get(api::mode))
        .route("/api/login", post(api::login))
        .merge(protected)
        .layer(axum::extract::DefaultBodyLimit::max(16_384))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}

async fn security_headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert("content-security-policy", HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("microphone=(self)"),
    );
    response
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}
async fn javascript() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../static/app.js"),
    )
}
async fn audio_javascript() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../static/audio.js"),
    )
}
async fn i18n_javascript() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../static/i18n.js"),
    )
}
async fn worklet() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../static/worklet.js"),
    )
}
async fn stylesheet() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../static/style.css"),
    )
}
