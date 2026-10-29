use crate::{app, auth, crypto, tests::state};
use axum::http::header;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use url::Url;

#[tokio::test]
async fn a_revoked_waiting_socket_cannot_connect_to_a_provider() {
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut state = state();
    let mut config = (*state.config).clone();
    config.gemini_url =
        Url::parse(&format!("ws://{}/live", upstream.local_addr().unwrap())).unwrap();
    state.config = Arc::new(config);
    let owner = crypto::owner_id("builtin", "alice");
    let key = crypto::seal(&state.config.master_key, &owner, "gemini", "synthetic-key").unwrap();
    state.store.put_secret(&owner, "gemini", &key).unwrap();
    let user = auth::Identity {
        owner: owner.clone(),
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
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::COOKIE,
        format!("speeching_session={token}").parse().unwrap(),
    );
    state.auth.logout(&headers);
    client.send(Message::Text(json!({"provider":"gemini","language":"Spanish","level":"Beginner","voice":"Aoede","pace":"normal","input_mode":"voice","chat_id":null}).to_string().into())).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), client.next())
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(300), upstream.accept())
            .await
            .is_err()
    );
    assert!(state.store.list(&owner).unwrap().is_empty());
    server.abort();
}
