use crate::{
    auth::{self, Identity},
    crypto,
    protocol::{Event, Practice},
    store::Message as Transcript,
    AppState,
};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Extension, State,
    },
    http::StatusCode,
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Instant};
use tokio::{
    sync::OwnedSemaphorePermit,
    time::{timeout, Duration},
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        client::IntoClientRequest, protocol::WebSocketConfig, Message as UpstreamMessage,
    },
};

const MAX_AUDIO_CHUNK: usize = 8192;
mod provider;
mod recorder;
use recorder::Recorder;
#[cfg(test)]
mod key_tests;

pub async fn upgrade(
    State(state): State<AppState>,
    Extension(user): Extension<Identity>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    let permit = Arc::clone(&state.live_limit)
        .try_acquire_owned()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(ws
        .max_message_size(32_768)
        .max_frame_size(32_768)
        .on_upgrade(move |socket| run(socket, state, user, permit)))
}

async fn run(
    mut socket: WebSocket,
    state: AppState,
    user: Identity,
    _permit: OwnedSemaphorePermit,
) {
    let Some(practice) = read_start(&mut socket).await else {
        return;
    };
    if let Err(error) = practice.validate() {
        send_error(&mut socket, error).await;
        return;
    }
    if !auth::session_valid(&state, &user) {
        return;
    }
    let Some(key) = provider_key(&state, &user, &practice, &mut socket).await else {
        return;
    };
    if !auth::session_valid(&state, &user) {
        return;
    }
    let Some(mut upstream) = connect_provider(&state, &practice, &key, &mut socket).await else {
        return;
    };
    if !auth::session_valid(&state, &user) {
        return;
    }
    let Some((chat_id, history)) = prepare_chat(&state, &user, &practice, &mut socket).await else {
        return;
    };
    let history = practice.history(&history);
    let model = provider_model(&state, &practice);
    let setup = practice.setup(model, !history.is_empty());
    if !initialize_provider(&mut socket, &mut upstream, setup, &chat_id).await {
        return;
    }
    let mut persistence = Persistence {
        state,
        user,
        chat_id,
        session_id: crypto::random_token(18),
        started: Instant::now(),
    };
    let (transcript, client_tx) =
        relay(socket, upstream, &practice, &history, &mut persistence).await;
    finish_session(&persistence, &transcript, client_tx).await;
}

async fn initialize_provider(
    socket: &mut WebSocket,
    upstream: &mut ProviderSocket,
    setup: Value,
    chat_id: &str,
) -> bool {
    if !matches!(
        timeout(
            Duration::from_secs(3),
            upstream.send(UpstreamMessage::Text(setup.to_string().into()))
        )
        .await,
        Ok(Ok(()))
    ) {
        send_error(socket, "The provider connection closed during setup.").await;
        return false;
    }
    matches!(
        timeout(
            Duration::from_secs(3),
            socket.send(Message::Text(
                json!({"type":"chat","id":chat_id}).to_string().into(),
            )),
        )
        .await,
        Ok(Ok(()))
    )
}

fn provider_model<'a>(state: &'a AppState, practice: &Practice) -> &'a str {
    if practice.provider == "gemini" {
        &state.config.gemini_model
    } else {
        &state.config.openai_model
    }
}

async fn finish_session(
    persistence: &Persistence,
    transcript: &[Transcript],
    mut client_tx: ClientSink,
) {
    if let Err(error) = persistence.save(transcript) {
        tracing::warn!(%error, "could not save conversation");
        let _ = timeout(
            Duration::from_secs(3),
            client_tx.send(Message::Text(
                json!({"type":"error","message":"The conversation could not be saved."})
                    .to_string()
                    .into(),
            )),
        )
        .await;
    } else {
        let _ = timeout(
            Duration::from_secs(3),
            client_tx.send(Message::Text(json!({"type":"saved"}).to_string().into())),
        )
        .await;
    }
    let _ = timeout(Duration::from_secs(3), client_tx.close()).await;
}

async fn read_start(socket: &mut WebSocket) -> Option<Practice> {
    let Ok(Some(Ok(Message::Text(text)))) = timeout(Duration::from_secs(15), socket.next()).await
    else {
        return None;
    };
    serde_json::from_str::<Practice>(&text).ok()
}

async fn prepare_chat(
    state: &AppState,
    user: &Identity,
    p: &Practice,
    socket: &mut WebSocket,
) -> Option<(String, Vec<Transcript>)> {
    if let Some(id) = &p.chat_id {
        match state.store.messages(&user.owner, id) {
            Ok(Some(history)) => return Some((id.clone(), history)),
            _ => {
                send_error(socket, "That conversation is unavailable.").await;
                return None;
            }
        }
    }
    match state.store.create(&user.owner, &p.language) {
        Ok(id) => Some((id, Vec::new())),
        Err(_) => {
            send_error(socket, "Could not create the conversation.").await;
            None
        }
    }
}

async fn provider_key(
    state: &AppState,
    user: &Identity,
    p: &Practice,
    socket: &mut WebSocket,
) -> Option<String> {
    match load_key(state, user, p) {
        Ok(Some(key)) => Some(key),
        Ok(None) => {
            send_error(
                socket,
                "Add a provider API key in settings before starting.",
            )
            .await;
            None
        }
        Err(message) => {
            send_error(socket, message).await;
            None
        }
    }
}

fn load_key(
    state: &AppState,
    user: &Identity,
    p: &Practice,
) -> Result<Option<String>, &'static str> {
    let saved = state
        .store
        .secret(&user.owner, &p.provider)
        .map_err(|_| "The data store is unavailable.")?;
    saved.map(|value| crypto::open(&state.config.master_key, &user.owner, &p.provider, &value).map_err(|_| "The saved key cannot be decrypted. Check the server master key or replace the provider key.")).transpose()
}

type ProviderSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect_provider(
    state: &AppState,
    p: &Practice,
    key: &str,
    socket: &mut WebSocket,
) -> Option<ProviderSocket> {
    let mut url = if p.provider == "gemini" {
        state.config.gemini_url.clone()
    } else {
        state.config.openai_url.clone()
    };
    if p.provider == "gemini" {
        url.query_pairs_mut().append_pair("key", key);
    } else {
        url.query_pairs_mut()
            .append_pair("model", &state.config.openai_model);
    }
    let Ok(mut request) = url.as_str().into_client_request() else {
        send_error(socket, "Invalid provider configuration.").await;
        return None;
    };
    if p.provider == "openai" {
        let Ok(value) = format!("Bearer {key}").parse() else {
            send_error(socket, "Invalid provider API key.").await;
            return None;
        };
        request.headers_mut().insert("Authorization", value);
    }
    let config = WebSocketConfig::default()
        .max_message_size(Some(2 * 1024 * 1024))
        .max_frame_size(Some(2 * 1024 * 1024));
    match timeout(
        Duration::from_secs(20),
        connect_async_with_config(request, Some(config), false),
    )
    .await
    {
        Ok(Ok((connection, _))) => Some(connection),
        Ok(Err(error)) => {
            provider::connection_failed(socket, p, Some(&error)).await;
            None
        }
        Err(_) => {
            provider::connection_failed(socket, p, None).await;
            None
        }
    }
}

async fn send_error(socket: &mut WebSocket, message: &str) {
    let _ = timeout(
        Duration::from_secs(3),
        socket.send(Message::Text(
            json!({"type":"error","message":message}).to_string().into(),
        )),
    )
    .await;
}

struct Persistence {
    state: AppState,
    user: Identity,
    chat_id: String,
    session_id: String,
    started: Instant,
}
impl Persistence {
    fn save(&self, messages: &[Transcript]) -> Result<(), String> {
        self.state.store.checkpoint(
            &self.user.owner,
            &self.chat_id,
            &self.session_id,
            messages,
            self.started.elapsed().as_secs(),
        )
    }
    fn authorized(&self) -> bool {
        auth::session_valid(&self.state, &self.user)
    }
}

async fn relay(
    socket: WebSocket,
    upstream: ProviderSocket,
    practice: &Practice,
    history: &[Value],
    persistence: &mut Persistence,
) -> (
    Vec<Transcript>,
    futures_util::stream::SplitSink<WebSocket, Message>,
) {
    let (mut client_tx, mut client_rx) = socket.split();
    let (mut provider_tx, mut provider_rx) = upstream.split();
    let mut recorder = Recorder::new();
    let mut ready = false;
    let deadline = tokio::time::sleep(Duration::from_secs(60 * 60));
    tokio::pin!(deadline);
    let ready_deadline = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(ready_deadline);
    let mut authorization_check = tokio::time::interval(Duration::from_secs(1));
    loop {
        let event = tokio::select! {
            _ = &mut deadline => RelayTick::Deadline,
            _ = authorization_check.tick() => RelayTick::Authorization,
            _ = &mut ready_deadline, if !ready => RelayTick::SetupTimeout,
            incoming = client_rx.next() => RelayTick::Client(incoming),
            incoming = provider_rx.next() => RelayTick::Provider(incoming),
        };
        let mut receiver = Receiver {
            practice,
            history,
            persistence,
            provider_tx: &mut provider_tx,
            client_tx: &mut client_tx,
            recorder: &mut recorder,
            ready: &mut ready,
        };
        if !handle_tick(event, &mut receiver).await {
            break;
        }
    }
    (recorder.messages, client_tx)
}

type ClientSink = futures_util::stream::SplitSink<WebSocket, Message>;
type ProviderSink = futures_util::stream::SplitSink<ProviderSocket, UpstreamMessage>;
enum RelayTick {
    Deadline,
    Authorization,
    SetupTimeout,
    Client(Option<Result<Message, axum::Error>>),
    Provider(Option<Result<UpstreamMessage, tokio_tungstenite::tungstenite::Error>>),
}
struct Receiver<'a> {
    practice: &'a Practice,
    history: &'a [Value],
    persistence: &'a Persistence,
    provider_tx: &'a mut ProviderSink,
    client_tx: &'a mut ClientSink,
    recorder: &'a mut Recorder,
    ready: &'a mut bool,
}

async fn handle_tick(event: RelayTick, receiver: &mut Receiver<'_>) -> bool {
    match event {
        RelayTick::Deadline => false,
        RelayTick::Authorization => receiver.persistence.authorized(),
        RelayTick::SetupTimeout => {
            client_error(
                receiver.client_tx,
                "The provider did not finish setup in time.",
            )
            .await;
            false
        }
        RelayTick::Client(incoming) => receive_client(incoming, receiver).await,
        RelayTick::Provider(incoming) => provider::receive(incoming, receiver).await,
    }
}

async fn receive_client(
    incoming: Option<Result<Message, axum::Error>>,
    receiver: &mut Receiver<'_>,
) -> bool {
    let Some(Ok(message)) = incoming else {
        return false;
    };
    forward_client(
        message,
        receiver.practice,
        receiver.provider_tx,
        *receiver.ready,
    )
    .await
}

async fn receive_event(event: Event, receiver: &mut Receiver<'_>) -> bool {
    if event == Event::Ready && !*receiver.ready {
        *receiver.ready = true;
        if !send_history(receiver.provider_tx, receiver.history).await {
            return false;
        }
    }
    let turn_finished = event == Event::TurnEnd;
    if !forward_event(event, receiver.client_tx, receiver.recorder).await {
        return false;
    }
    if turn_finished
        && receiver
            .persistence
            .save(&receiver.recorder.messages)
            .is_err()
    {
        client_error(
            receiver.client_tx,
            "A conversation checkpoint failed; saving will be retried.",
        )
        .await;
    }
    true
}

async fn send_history(tx: &mut ProviderSink, history: &[Value]) -> bool {
    for item in history {
        if !matches!(
            timeout(
                Duration::from_secs(3),
                tx.send(UpstreamMessage::Text(item.to_string().into()))
            )
            .await,
            Ok(Ok(()))
        ) {
            return false;
        }
    }
    true
}

async fn client_error(tx: &mut ClientSink, message: &str) {
    let _ = timeout(
        Duration::from_secs(3),
        tx.send(Message::Text(
            json!({"type":"error","message":message}).to_string().into(),
        )),
    )
    .await;
}

async fn forward_client(
    message: Message,
    practice: &Practice,
    tx: &mut futures_util::stream::SplitSink<ProviderSocket, UpstreamMessage>,
    ready: bool,
) -> bool {
    if matches!(&message, Message::Close(_))
        || matches!(&message, Message::Text(text) if *text == "stop")
    {
        return false;
    }
    if !ready {
        return true;
    }
    let payloads = client_payloads(message, practice);
    for payload in payloads {
        if !matches!(
            timeout(
                Duration::from_secs(3),
                tx.send(UpstreamMessage::Text(payload.to_string().into()))
            )
            .await,
            Ok(Ok(()))
        ) {
            return false;
        }
    }
    true
}

fn client_payloads(message: Message, practice: &Practice) -> Vec<Value> {
    match message {
        Message::Binary(bytes) => audio_payload(practice, &bytes),
        Message::Text(text) => control_payload(practice, &text),
        _ => Vec::new(),
    }
}

fn audio_payload(practice: &Practice, bytes: &[u8]) -> Vec<Value> {
    if bytes.len() > MAX_AUDIO_CHUNK || !bytes.len().is_multiple_of(2) {
        return Vec::new();
    }
    vec![practice.audio(bytes)]
}

fn control_payload(practice: &Practice, text: &str) -> Vec<Value> {
    if practice.input_mode != "push" {
        return Vec::new();
    }
    match text {
        "press" => practice.activity(true),
        "release" => practice.activity(false),
        _ => Vec::new(),
    }
}

async fn forward_event(
    event: Event,
    tx: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    recorder: &mut Recorder,
) -> bool {
    let message = match event {
        Event::Ready => json!({"type":"ready"}),
        Event::UserItem(id) => {
            if !recorder.reserve_user(&id) {
                client_error(tx, "This session reached its transcript limit. Start again to continue in the same conversation.").await;
                return false;
            }
            json!({"type":"user_item","id":id})
        }
        Event::Audio(bytes) => {
            return matches!(
                timeout(
                    Duration::from_secs(3),
                    tx.send(Message::Binary(bytes.into()))
                )
                .await,
                Ok(Ok(()))
            )
        }
        Event::Text {
            speaker,
            text,
            id,
            replace,
        } => {
            if !recorder.add(speaker, &text, id.clone(), replace) {
                client_error(tx, "This session reached its transcript limit. Start again to continue in the same conversation.").await;
                return false;
            }
            json!({"type":"transcript","speaker":speaker,"text":text,"id":id,"replace":replace})
        }
        Event::Interrupted => json!({"type":"interrupted"}),
        Event::TurnEnd => json!({"type":"turn_end"}),
        Event::Error => {
            json!({"type":"error","message":"The provider reported an error. Check the configured model and key."})
        }
    };
    matches!(
        timeout(
            Duration::from_secs(3),
            tx.send(Message::Text(message.to_string().into()))
        )
        .await,
        Ok(Ok(()))
    )
}
