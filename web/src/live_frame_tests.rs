use crate::{app, config::AuthMode, crypto, tests};
use axum::http::header;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{net::TcpListener, time::timeout};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    protocol::{frame::coding::CloseCode, CloseFrame},
    Message,
};
use url::Url;

type TestClient =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect_test_app(
    provider_address: SocketAddr,
) -> (TestClient, tokio::task::JoinHandle<()>) {
    let mut state = tests::state();
    let mut config = (*state.config).clone();
    config.auth = AuthMode::Off;
    config.gemini_url = Url::parse(&format!("ws://{provider_address}/live")).unwrap();
    state.config = Arc::new(config);
    let owner = crypto::owner_id("off", "guest");
    let key = crypto::seal(
        &state.config.master_key,
        &owner,
        "gemini",
        "synthetic-test-key",
    )
    .unwrap();
    state.store.put_secret(&owner, "gemini", &key).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<SocketAddr>(),
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
    let (mut client, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let practice = json!({"provider":"gemini","language":"Spanish","level":"Beginner","voice":"Aoede","pace":"slow","input_mode":"voice","chat_id":null});
    client
        .send(Message::Text(practice.to_string().into()))
        .await
        .unwrap();
    let first = next_json(&mut client).await;
    assert_eq!(first["type"], "chat");
    (client, server)
}

async fn next_json(client: &mut TestClient) -> Value {
    let frame = timeout(Duration::from_secs(5), client.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}

#[tokio::test]
async fn binary_google_frames_start_and_relay_the_conversation() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let provider = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        socket.next().await.unwrap().unwrap();
        socket
            .send(Message::Binary(vec![0xff, 0x00].into()))
            .await
            .unwrap();
        socket
            .send(Message::Binary(
                json!({"setupComplete":{}}).to_string().into_bytes().into(),
            ))
            .await
            .unwrap();
        socket
            .send(Message::Binary(
                json!({"serverContent":{"outputTranscription":{"text":"Hola"},"modelTurn":{"parts":[{"inlineData":{"data":"AAE="}}]}}})
                    .to_string()
                    .into_bytes()
                    .into(),
            ))
            .await
            .unwrap();
        let _ = done_rx.await;
    });
    let (mut client, server) = connect_test_app(address).await;
    assert_eq!(next_json(&mut client).await["type"], "ready");
    let transcript = next_json(&mut client).await;
    assert_eq!(transcript["type"], "transcript");
    assert_eq!(transcript["text"], "Hola");
    let audio = timeout(Duration::from_secs(5), client.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(audio.into_data().as_ref(), &[0, 1]);
    let _ = done_tx.send(());
    provider.await.unwrap();
    server.abort();
}

#[tokio::test]
async fn rejected_provider_setup_reports_a_safe_actionable_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let provider = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        socket.next().await.unwrap().unwrap();
        socket
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Invalid,
                reason: "API key not valid. synthetic-secret-marker".into(),
            })))
            .await
            .unwrap();
    });
    let (mut client, server) = connect_test_app(address).await;
    let error = next_json(&mut client).await;
    assert_eq!(error["type"], "error");
    assert_eq!(error["code"], "providerInvalidKey");
    assert!(!error.to_string().contains("synthetic-secret-marker"));
    provider.await.unwrap();
    server.abort();
}
