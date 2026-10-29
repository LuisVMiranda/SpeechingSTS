use crate::{
    app, auth,
    config::{AuthMode, Config},
    crypto,
    store::Store,
    AppState,
};
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{header, Request, StatusCode},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{net::SocketAddr, path::Path, sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message as WsMessage};
use tower::ServiceExt;
use url::Url;

pub(crate) fn state() -> AppState {
    let config = Config {
        listen: "127.0.0.1:8787".parse().unwrap(),
        origin: "http://127.0.0.1:8787".into(),
        auth: AuthMode::Builtin,
        allow_registration: true,
        trusted_proxy: "127.0.0.1".parse().unwrap(),
        trust_client_ip_header: false,
        proxy_secret: None,
        data_dir: "unused".into(),
        master_key: [7; 32],
        gemini_url: Url::parse("wss://example.com/live").unwrap(),
        openai_url: Url::parse("wss://example.com/live").unwrap(),
        gemini_model: "test".into(),
        openai_model: "test".into(),
        max_sessions: 2,
        cooldown_seconds: 300,
    };
    let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
    store
        .put_user("alice", &auth::hash_password("alice-password-123").unwrap())
        .unwrap();
    store
        .put_user("bob", &auth::hash_password("bob-password-12345").unwrap())
        .unwrap();
    AppState {
        config: Arc::new(config),
        store,
        auth: Arc::new(auth::AuthState::new(Duration::from_secs(300))),
        live_limit: Arc::new(Semaphore::new(2)),
        password_limit: Arc::new(Semaphore::new(2)),
        dummy_hash: auth::hash_password("not-a-real-user-password").unwrap(),
    }
}

pub(crate) async fn request(
    app: &Router,
    method: &str,
    path: &str,
    headers: (Option<&str>, Option<&str>),
    body: Value,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(value) = headers.0 {
        builder = builder.header(header::ORIGIN, value);
    }
    if let Some(value) = headers.1 {
        builder = builder.header(header::COOKIE, value);
    }
    let mut request = builder.body(Body::from(body.to_string())).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()));
    app.clone().oneshot(request).await.unwrap()
}

pub(crate) async fn login(
    app: &Router,
    username: &str,
    password: &str,
) -> axum::response::Response {
    request(
        app,
        "POST",
        "/api/login",
        (Some("http://127.0.0.1:8787"), None),
        json!({"username":username,"password":password}),
    )
    .await
}

#[tokio::test]
async fn endpoints_enforce_identity_csrf_secrets_and_cooldown() {
    let state = state();
    let id = state
        .store
        .create(&crypto::owner_id("builtin", "alice"), "Spanish")
        .unwrap();
    let app = app(state.clone());
    let permits = state
        .password_limit
        .clone()
        .acquire_many_owned(2)
        .await
        .unwrap();
    for _ in 0..3 {
        assert_eq!(
            login(&app, "alice", "wrong-password").await.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
    drop(permits);
    assert_eq!(
        request(&app, "GET", "/api/me", (None, None), json!({}))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/login",
            (Some("https://evil.example"), None),
            json!({"username":"alice","password":"alice-password-123"})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    // The dummy hash must never authenticate an account that does not exist.
    assert_eq!(
        login(&app, "unknown", "not-a-real-user-password")
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = login(&app, "alice", "alice-password-123").await;
    assert_eq!(response.status(), StatusCode::OK);
    let alice_cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/keys/gemini",
            (Some("https://evil.example"), Some(&alice_cookie)),
            json!({"key":"secret"})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/keys/gemini",
            (Some("http://127.0.0.1:8787"), Some(&alice_cookie)),
            json!({"key":"example-secret"})
        )
        .await
        .status(),
        StatusCode::OK
    );
    let me = request(
        &app,
        "GET",
        "/api/me",
        (None, Some(&alice_cookie)),
        json!({}),
    )
    .await;
    let body = to_bytes(me.into_body(), 10_000).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("example-secret"));
    let bob = login(&app, "bob", "bob-password-12345").await;
    let bob_cookie = bob.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/api/chats/{id}"),
            (None, Some(&bob_cookie)),
            json!({})
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/logout",
            (Some("http://127.0.0.1:8787"), Some(&bob_cookie)),
            json!({})
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "GET", "/api/me", (None, Some(&bob_cookie)), json!({}))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for _ in 0..3 {
        assert_eq!(
            login(&app, "alice", "wrong-password").await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        login(&app, "alice", "alice-password-123").await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    state
        .store
        .put_user(
            "alice",
            &auth::hash_password("new-alice-password-123").unwrap(),
        )
        .unwrap();
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/me",
            (None, Some(&alice_cookie)),
            json!({})
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn proxy_identity_requires_the_configured_peer() {
    let mut state = state();
    let mut config = (*state.config).clone();
    config.auth = AuthMode::Proxy;
    config.proxy_secret = Some("synthetic-proxy-secret-for-test-only".into());
    state.config = Arc::new(config);
    let app = app(state);
    let mut missing = Request::builder()
        .uri("/api/me")
        .header("x-speeching-user", "alice")
        .body(Body::empty())
        .unwrap();
    missing
        .extensions_mut()
        .insert(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()));
    assert_eq!(
        app.clone().oneshot(missing).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    for (ip, secret, status) in [
        (
            "127.0.0.1:1234",
            "synthetic-proxy-secret-for-test-only",
            StatusCode::OK,
        ),
        (
            "127.0.0.2:1234",
            "synthetic-proxy-secret-for-test-only",
            StatusCode::UNAUTHORIZED,
        ),
        ("127.0.0.1:1234", "wrong-secret", StatusCode::UNAUTHORIZED),
    ] {
        let mut request = Request::builder()
            .uri("/api/me")
            .header("x-speeching-user", "alice")
            .header("x-speeching-proxy-secret", secret)
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(ip.parse::<SocketAddr>().unwrap()));
        assert_eq!(app.clone().oneshot(request).await.unwrap().status(), status);
    }
    let mut request = Request::builder()
        .uri("/api/me")
        .header("x-speeching-user", "bad user")
        .header(
            "x-speeching-proxy-secret",
            "synthetic-proxy-secret-for-test-only",
        )
        .body(Body::empty())
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()));
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn live_upgrade_rejects_missing_auth_and_wrong_origin() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = state();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    for (origin, expected) in [
        ("http://127.0.0.1:8787", StatusCode::UNAUTHORIZED),
        ("https://evil.example", StatusCode::FORBIDDEN),
    ] {
        let mut request = format!("ws://{address}/api/live")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert(header::ORIGIN, origin.parse().unwrap());
        let result = tokio_tungstenite::connect_async(request).await;
        assert!(
            matches!(result, Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status() == expected)
        );
    }
    server.abort();
}

#[tokio::test]
async fn signing_out_revokes_an_active_live_socket() {
    let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let provider = tokio::spawn(async move {
        let (stream, _) = upstream_listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        socket.next().await.unwrap().unwrap();
        socket
            .send(WsMessage::Text(
                json!({"setupComplete":{}}).to_string().into(),
            ))
            .await
            .unwrap();
        while socket.next().await.is_some() {}
    });
    let mut state = state();
    let mut config = (*state.config).clone();
    config.gemini_url = Url::parse(&format!("ws://{upstream_address}/live")).unwrap();
    state.config = Arc::new(config);
    let owner = crypto::owner_id("builtin", "alice");
    let sealed = crypto::seal(&state.config.master_key, &owner, "gemini", "fake-key").unwrap();
    state.store.put_secret(&owner, "gemini", &sealed).unwrap();
    let user = auth::Identity {
        owner,
        name: "alice".into(),
        session_token: None,
        password_hash: state.store.password_hash("alice").unwrap(),
    };
    let token = state.auth.create_session(user).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_state = state.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app(server_state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let mut request = format!("ws://{address}/api/live")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(header::ORIGIN, "http://127.0.0.1:8787".parse().unwrap());
    request.headers_mut().insert(
        header::COOKIE,
        format!("speeching_session={token}").parse().unwrap(),
    );
    let (mut client, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    client.send(WsMessage::Text(json!({"provider":"gemini","language":"Spanish","level":"Beginner","voice":"Aoede","pace":"slow","input_mode":"voice","chat_id":null}).to_string().into())).await.unwrap();
    client.next().await.unwrap().unwrap();
    client.next().await.unwrap().unwrap();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::COOKIE,
        format!("speeching_session={token}").parse().unwrap(),
    );
    state.auth.logout(&headers);
    tokio::time::timeout(Duration::from_secs(4), async {
        while let Some(Ok(message)) = client.next().await {
            if message.is_close() {
                break;
            }
        }
    })
    .await
    .expect("socket should close after logout");
    server.abort();
    provider.abort();
}

#[tokio::test]
async fn live_relay_restores_history_and_saves_transcripts_for_both_providers() {
    for provider in ["gemini", "openai"] {
        exercise_relay(provider).await;
    }
}

async fn exercise_relay(provider: &str) {
    let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let provider_name = provider.to_owned();
    let mock = tokio::spawn(async move {
        let (stream, _) = upstream_listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let setup = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(!setup.contains("history-only-marker"));
        let ready = if provider_name == "gemini" {
            json!({"setupComplete":{}})
        } else {
            json!({"type":"session.updated"})
        };
        ws.send(WsMessage::Text(ready.to_string().into()))
            .await
            .unwrap();
        let history = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(history.contains("history-only-marker"));
        let chunk = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(chunk.contains("AQI="));
        let reply = if provider_name == "gemini" {
            json!({"serverContent":{"inputTranscription":{"text":"A new learner sentence"},"outputTranscription":{"text":"Hello again"},"modelTurn":{"parts":[{"inlineData":{"data":"AAE="}}]},"turnComplete":true}})
        } else {
            json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"one","transcript":"A new learner sentence"})
        };
        ws.send(WsMessage::Text(reply.to_string().into()))
            .await
            .unwrap();
        if provider_name == "openai" {
            ws.send(WsMessage::Text(
                json!({"type":"response.output_audio.delta","delta":"AAE="})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        }
    });
    let mut state = state();
    let mut config = (*state.config).clone();
    config.auth = AuthMode::Off;
    let address = format!("ws://{upstream_address}/live");
    config.gemini_url = Url::parse(&address).unwrap();
    config.openai_url = Url::parse(&address).unwrap();
    state.config = Arc::new(config);
    let owner = crypto::owner_id("off", "guest");
    let sealed = crypto::seal(
        &state.config.master_key,
        &owner,
        provider,
        "test-provider-key",
    )
    .unwrap();
    state.store.put_secret(&owner, provider, &sealed).unwrap();
    let id = state.store.create(&owner, "Spanish").unwrap();
    state
        .store
        .append_session(
            &owner,
            &id,
            &[crate::store::Message {
                speaker: "user".into(),
                text: "history-only-marker".into(),
            }],
            1,
        )
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_address = listener.local_addr().unwrap();
    let server_state = state.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app(server_state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let mut malicious = format!("ws://{server_address}/api/live")
        .into_client_request()
        .unwrap();
    malicious
        .headers_mut()
        .insert(header::ORIGIN, "https://evil.example".parse().unwrap());
    let rejected = tokio_tungstenite::connect_async(malicious).await;
    assert!(
        matches!(rejected, Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status() == StatusCode::FORBIDDEN)
    );
    let mut request = format!("ws://{server_address}/api/live")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(header::ORIGIN, "http://127.0.0.1:8787".parse().unwrap());
    let (mut client, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let voice = if provider == "gemini" {
        "Aoede"
    } else {
        "marin"
    };
    client.send(WsMessage::Text(json!({"provider":provider,"language":"Spanish","level":"Beginner","voice":voice,"pace":"slow","input_mode":"voice","chat_id":id}).to_string().into())).await.unwrap();
    let first = client.next().await.unwrap().unwrap().into_text().unwrap();
    assert!(first.contains("\"chat\""));
    let ready = client.next().await.unwrap().unwrap().into_text().unwrap();
    assert!(ready.contains("\"ready\""));
    client
        .send(WsMessage::Binary(vec![1, 2].into()))
        .await
        .unwrap();
    let transcript = client.next().await.unwrap().unwrap().into_text().unwrap();
    assert!(transcript.contains("A new learner sentence"));
    loop {
        let message = client.next().await.unwrap().unwrap();
        if message.is_binary() {
            assert_eq!(message.into_data().as_ref(), &[0, 1]);
            break;
        }
    }
    mock.await.unwrap();
    for _ in 0..100 {
        let saved = state.store.messages(&owner, &id).unwrap().unwrap();
        if saved
            .iter()
            .any(|message| message.text == "A new learner sentence")
        {
            server.abort();
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    server.abort();
    panic!("Relay did not persist the new transcript");
}
