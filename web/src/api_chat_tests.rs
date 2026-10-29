use crate::{
    app, crypto,
    tests::{login, request, state},
};
use axum::http::{header, StatusCode};
use serde_json::json;

async fn cookie(app: &axum::Router, name: &str, password: &str) -> String {
    let response = login(app, name, password).await;
    response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn rename_and_delete_enforce_ownership_origin_and_validation() {
    let state = state();
    let id = state
        .store
        .create(&crypto::owner_id("builtin", "alice"), "Spanish")
        .unwrap();
    let app = app(state.clone());
    let alice = cookie(&app, "alice", "alice-password-123").await;
    let bob = cookie(&app, "bob", "bob-password-12345").await;
    let path = format!("/api/chats/{id}");
    let origin = Some("http://127.0.0.1:8787");
    assert_eq!(
        request(
            &app,
            "PATCH",
            &path,
            (Some("https://evil.example"), Some(&alice)),
            json!({"title":"bad"})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "PATCH",
            &path,
            (origin, Some(&alice)),
            json!({"title":" "})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &app,
            "PATCH",
            &path,
            (origin, Some(&bob)),
            json!({"title":"stolen"})
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&app, "DELETE", &path, (origin, Some(&bob)), json!({}))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            "PATCH",
            &path,
            (origin, Some(&alice)),
            json!({"title":"A better name"})
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        state
            .store
            .chat(&crypto::owner_id("builtin", "alice"), &id)
            .unwrap()
            .unwrap()
            .title,
        "A better name"
    );
    assert_eq!(
        request(&app, "DELETE", &path, (origin, Some(&alice)), json!({}))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "GET", &path, (None, Some(&alice)), json!({}))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}
