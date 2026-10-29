use crate::{app, auth, config::AuthMode, crypto, tests};
use axum::{
    body::to_bytes,
    http::{header, StatusCode},
};
use serde_json::{json, Value};
use std::sync::Arc;

async fn register(
    app: &axum::Router,
    name: &str,
    password: &str,
    origin: &str,
) -> axum::response::Response {
    tests::request(
        app,
        "POST",
        "/api/register",
        (Some(origin), None),
        json!({"username":name,"password":password}),
    )
    .await
}

#[tokio::test]
async fn registration_creates_an_isolated_account_without_replacing_existing_credentials() {
    let state = tests::state();
    let original_hash = state.store.password_hash("alice").unwrap().unwrap();
    let app = app(state.clone());
    let origin = "http://127.0.0.1:8787";
    assert_eq!(
        register(
            &app,
            "alice",
            "replacement-password",
            "https://attacker.example"
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        register(&app, "  ALICE  ", "replacement-password", origin)
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        state.store.password_hash("alice").unwrap().unwrap(),
        original_hash
    );
    assert_eq!(
        register(&app, " New.User ", "new-user-password-123", origin)
            .await
            .status(),
        StatusCode::CREATED
    );
    let hash = state.store.password_hash("new.user").unwrap().unwrap();
    assert_ne!(hash, "new-user-password-123");
    assert!(auth::verify_password("new-user-password-123", &hash));
    let response = tests::login(&app, "NEW.USER", "new-user-password-123").await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let owner = crypto::owner_id("builtin", "alice");
    state.store.create(&owner, "Spanish").unwrap();
    let response =
        tests::request(&app, "GET", "/api/chats", (None, Some(&cookie)), json!({})).await;
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16_384).await.unwrap()).unwrap();
    assert!(body["chats"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn concurrent_registration_cannot_overwrite_a_new_account() {
    let state = tests::state();
    let app = app(state.clone());
    let origin = "http://127.0.0.1:8787";
    let (first, second) = tokio::join!(
        register(&app, "same", "first-password-123", origin),
        register(&app, "same", "second-password-123", origin),
    );
    let statuses = [first.status(), second.status()];
    assert!(statuses.contains(&StatusCode::CREATED));
    assert!(statuses.contains(&StatusCode::CONFLICT));
    let hash = state.store.password_hash("same").unwrap().unwrap();
    assert_ne!(
        auth::verify_password("first-password-123", &hash),
        auth::verify_password("second-password-123", &hash)
    );
}

#[tokio::test]
async fn registration_is_limited_and_can_be_disabled() {
    let state = tests::state();
    let app = app(state.clone());
    let origin = "http://127.0.0.1:8787";
    assert_eq!(
        register(&app, "bad name", "password-long-enough", origin)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        register(&app, "valid", "short", origin).await.status(),
        StatusCode::BAD_REQUEST
    );
    for name in ["first", "second", "third"] {
        assert_eq!(
            register(&app, name, "password-long-enough", origin)
                .await
                .status(),
            StatusCode::CREATED
        );
    }
    assert_eq!(
        register(&app, "fourth", "password-long-enough", origin)
            .await
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        tests::login(&app, "alice", "alice-password-123")
            .await
            .status(),
        StatusCode::OK
    );

    let mut disabled = tests::state();
    let mut config = (*disabled.config).clone();
    config.allow_registration = false;
    disabled.config = Arc::new(config.clone());
    let disabled = crate::app(disabled);
    assert_eq!(
        register(&disabled, "new", "password-long-enough", origin)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let mode = tests::request(&disabled, "GET", "/api/mode", (None, None), json!({})).await;
    let body: Value =
        serde_json::from_slice(&to_bytes(mode.into_body(), 16_384).await.unwrap()).unwrap();
    assert_eq!(body["registration"], false);
    config.allow_registration = true;
    config.auth = AuthMode::Off;
    let mut guest = tests::state();
    guest.config = Arc::new(config);
    assert_eq!(
        register(&crate::app(guest), "new", "password-long-enough", origin)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn overloaded_password_workers_do_not_consume_registration_attempts() {
    let state = tests::state();
    let app = app(state.clone());
    let permits = state
        .password_limit
        .clone()
        .acquire_many_owned(2)
        .await
        .unwrap();
    for _ in 0..4 {
        assert_eq!(
            register(
                &app,
                "newuser",
                "password-long-enough",
                "http://127.0.0.1:8787"
            )
            .await
            .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
    drop(permits);
    assert_eq!(
        register(
            &app,
            "newuser",
            "password-long-enough",
            "http://127.0.0.1:8787"
        )
        .await
        .status(),
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn administrator_reset_keeps_the_existing_conversation_library() {
    let state = tests::state();
    let owner = crypto::owner_id("builtin", "alice");
    state.store.create(&owner, "Spanish").unwrap();
    state
        .store
        .put_user(
            "alice",
            &auth::hash_password("replacement-password-123").unwrap(),
        )
        .unwrap();
    let app = app(state);
    let login = tests::login(&app, "alice", "replacement-password-123").await;
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let response =
        tests::request(&app, "GET", "/api/chats", (None, Some(&cookie)), json!({})).await;
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16_384).await.unwrap()).unwrap();
    assert_eq!(body["chats"].as_array().unwrap().len(), 1);
}
