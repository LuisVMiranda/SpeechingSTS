use crate::{
    app, crypto,
    store::Message,
    tests::{login, request, state},
};
use axum::{
    body::to_bytes,
    http::{header, StatusCode},
};
use serde_json::{json, Value};

async fn response_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap()
}

#[tokio::test]
async fn changed_history_rejects_stale_pages_instead_of_skipping_or_duplicating() {
    let state = state();
    let owner = crypto::owner_id("builtin", "alice");
    for _ in 0..101 {
        state.store.create(&owner, "Spanish").unwrap();
    }
    let id = state.store.create(&owner, "English").unwrap();
    let mut messages: Vec<_> = (0..101)
        .map(|index| Message {
            speaker: "user".into(),
            text: format!("message {index}"),
        })
        .collect();
    state
        .store
        .checkpoint(&owner, &id, "session", &messages, 1)
        .unwrap();
    let app = app(state.clone());
    let login = login(&app, "alice", "alice-password-123").await;
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let headers = (None, Some(cookie.as_str()));
    let chats = response_json(request(&app, "GET", "/api/chats", headers, json!({})).await).await;
    let first_messages =
        response_json(request(&app, "GET", &format!("/api/chats/{id}"), headers, json!({})).await)
            .await;
    assert_eq!(chats["more"], true);
    assert_eq!(first_messages["more"], true);
    let revision = chats["revision"].as_i64().unwrap();
    assert_eq!(first_messages["revision"], revision);
    messages.push(Message {
        speaker: "partner".into(),
        text: "new answer".into(),
    });
    state
        .store
        .checkpoint(&owner, &id, "session", &messages, 2)
        .unwrap();
    let stale_chats = format!("/api/chats?offset=100&revision={revision}");
    let stale_messages = format!("/api/chats/{id}?offset=100&revision={revision}");
    assert_eq!(
        request(&app, "GET", &stale_chats, headers, json!({}))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&app, "GET", &stale_messages, headers, json!({}))
            .await
            .status(),
        StatusCode::CONFLICT
    );
}
