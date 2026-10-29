use crate::{app, config::AuthMode, crypto, tests::state};
use axum::http::header;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{io::AsyncReadExt, net::TcpListener, time::timeout};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use url::Url;

#[tokio::test]
async fn secure_provider_handshake_reports_connection_failure_without_panicking() {
    crate::install_tls_provider().expect("TLS provider should initialize");
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let provider = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut record = [0; 5];
        timeout(Duration::from_secs(5), stream.read_exact(&mut record))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record[0], 0x16, "expected a TLS handshake record");
    });

    let mut state = state();
    let mut config = (*state.config).clone();
    config.auth = AuthMode::Off;
    config.gemini_url = Url::parse(&format!("wss://{upstream_address}/live")).unwrap();
    state.config = Arc::new(config);
    let owner = crypto::owner_id("off", "guest");
    let key = crypto::seal(&state.config.master_key, &owner, "gemini", "test-key").unwrap();
    state.store.put_secret(&owner, "gemini", &key).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let mut request = format!("ws://{server_address}/api/live")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(header::ORIGIN, "http://127.0.0.1:8787".parse().unwrap());
    let (mut client, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let practice = json!({"provider":"gemini","language":"Spanish","level":"Beginner","voice":"Aoede","pace":"slow","input_mode":"voice","chat_id":null});
    client
        .send(Message::Text(practice.to_string().into()))
        .await
        .unwrap();
    let response = timeout(Duration::from_secs(5), client.next())
        .await
        .expect("the connection failure should be reported")
        .expect("the live socket should send an error")
        .expect("the live socket should not fail")
        .into_text()
        .unwrap();
    let response: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(response["type"], "error");
    assert!(response["message"]
        .as_str()
        .unwrap()
        .contains("Could not connect"));
    provider.await.unwrap();
    server.abort();
}
