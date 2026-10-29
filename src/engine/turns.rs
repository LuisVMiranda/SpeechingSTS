use super::connection::Socket;
use super::{protocol, Command, ConnectionContext, Event, Incoming, InputMode, Provider};
use futures_util::SinkExt;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{Error, Message};

#[derive(Default)]
pub(super) struct LiveState {
    pub ready: bool,
    pub handle: Option<String>,
    talking: bool,
    talk_bytes: usize,
    response_active: bool,
    ignore_old_output: bool,
}

impl LiveState {
    pub(super) fn reset_connection(&mut self) {
        let handle = self.handle.take();
        *self = Self {
            handle,
            ..Self::default()
        };
    }
}

pub(super) enum MessageResult {
    Continue,
    Fatal,
    Disconnected(bool),
}

async fn send(socket: &mut Socket, value: serde_json::Value) -> bool {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .is_ok()
}

pub(super) async fn handle_command(
    context: &ConnectionContext,
    socket: &mut Socket,
    audio_rx: &mut mpsc::Receiver<Vec<u8>>,
    state: &mut LiveState,
    command: Option<Command>,
) -> bool {
    match command {
        Some(Command::Mute(muted)) => handle_mute(context, socket, state, muted).await,
        Some(Command::Talk(talking)) => {
            handle_talk(context, socket, audio_rx, state, talking).await
        }
        Some(Command::Stop) | None => {
            let _ = socket.close(None).await;
            return true;
        }
    }
    false
}

async fn handle_mute(
    context: &ConnectionContext,
    socket: &mut Socket,
    state: &LiveState,
    muted: bool,
) {
    if muted
        && state.ready
        && context.settings.provider == Provider::Gemini
        && context.settings.input_mode == InputMode::VoiceActivated
    {
        let _ = send(
            socket,
            serde_json::json!({"realtimeInput":{"audioStreamEnd":true}}),
        )
        .await;
    }
}

async fn handle_talk(
    context: &ConnectionContext,
    socket: &mut Socket,
    audio_rx: &mut mpsc::Receiver<Vec<u8>>,
    state: &mut LiveState,
    talking: bool,
) {
    if !state.ready {
        return;
    }
    if talking && context.settings.input_mode == InputMode::PushToTalk && !state.talking {
        start_talking(context, socket, state).await;
    } else if !talking && state.talking {
        finish_talking(context, socket, audio_rx, state).await;
    }
}

async fn start_talking(context: &ConnectionContext, socket: &mut Socket, state: &mut LiveState) {
    context.audio.clear_output();
    state.talk_bytes = 0;
    if context.settings.provider == Provider::Gemini {
        let _ = send(
            socket,
            serde_json::json!({"realtimeInput":{"activityStart":{}}}),
        )
        .await;
    } else {
        if state.response_active {
            state.ignore_old_output = true;
            let _ = send(socket, serde_json::json!({"type":"response.cancel"})).await;
        }
        let _ = send(
            socket,
            serde_json::json!({"type":"input_audio_buffer.clear"}),
        )
        .await;
    }
    state.talking = true;
}

async fn finish_talking(
    context: &ConnectionContext,
    socket: &mut Socket,
    audio_rx: &mut mpsc::Receiver<Vec<u8>>,
    state: &mut LiveState,
) {
    while let Ok(chunk) = audio_rx.try_recv() {
        state.talk_bytes += chunk.len();
        let _ = send(
            socket,
            protocol::audio_chunk(context.settings.provider, &chunk),
        )
        .await;
    }
    state.talking = false;
    if context.settings.provider == Provider::Gemini {
        let _ = send(
            socket,
            serde_json::json!({"realtimeInput":{"activityEnd":{}}}),
        )
        .await;
    } else if state.talk_bytes >= 4_800 {
        let _ = send(
            socket,
            serde_json::json!({"type":"input_audio_buffer.commit"}),
        )
        .await;
        let _ = send(socket, serde_json::json!({"type":"response.create"})).await;
    } else {
        let _ = send(
            socket,
            serde_json::json!({"type":"input_audio_buffer.clear"}),
        )
        .await;
    }
}

pub(super) async fn handle_chunk(
    context: &ConnectionContext,
    socket: &mut Socket,
    state: &mut LiveState,
    chunk: Vec<u8>,
) -> bool {
    let allowed = if context.settings.input_mode == InputMode::PushToTalk {
        state.talking
    } else {
        !context.audio.muted.load(Ordering::Relaxed)
    };
    if !state.ready || !allowed {
        return true;
    }
    if state.talking {
        state.talk_bytes += chunk.len();
    }
    send(
        socket,
        protocol::audio_chunk(context.settings.provider, &chunk),
    )
    .await
}

pub(super) async fn handle_message(
    context: &ConnectionContext,
    socket: &mut Socket,
    state: &mut LiveState,
    message: Option<Result<Message, Error>>,
) -> MessageResult {
    let text = match message {
        Some(Ok(Message::Text(text))) => text.to_string(),
        Some(Ok(Message::Binary(bytes))) => String::from_utf8_lossy(&bytes).into_owned(),
        Some(Ok(Message::Close(frame))) => {
            return MessageResult::Disconnected(
                frame
                    .as_ref()
                    .map(|f| u16::from(f.code) != 1008)
                    .unwrap_or(true),
            )
        }
        Some(Ok(_)) => return MessageResult::Continue,
        _ => return MessageResult::Disconnected(true),
    };
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text) else {
        return MessageResult::Continue;
    };
    update_response_state(context.settings.provider, state, &payload);
    for event in protocol::parse(context.settings.provider, &payload) {
        if !handle_incoming(context, socket, state, event).await {
            return MessageResult::Fatal;
        }
    }
    MessageResult::Continue
}

fn update_response_state(provider: Provider, state: &mut LiveState, payload: &serde_json::Value) {
    if provider != Provider::OpenAi {
        return;
    }
    match payload.get("type").and_then(serde_json::Value::as_str) {
        Some("response.created") => {
            state.response_active = true;
            state.ignore_old_output = false;
        }
        Some("response.done") => {
            state.response_active = false;
            state.ignore_old_output = false;
        }
        _ => {}
    }
}

async fn handle_incoming(
    context: &ConnectionContext,
    socket: &mut Socket,
    state: &mut LiveState,
    event: Incoming,
) -> bool {
    match event {
        Incoming::Ready => return handle_ready(context, socket, state).await,
        Incoming::Audio(bytes) => {
            queue_output(context, state, &bytes);
        }
        Incoming::Interrupt => {
            context.audio.clear_output();
            let _ = context.events.send(Event::TurnEnd);
        }
        Incoming::Resume(value) => state.handle = Some(value),
        Incoming::Transcript {
            speaker,
            text,
            id,
            replace,
        } => {
            let _ = context.events.send(Event::Transcript {
                speaker,
                text,
                id,
                replace,
            });
        }
        Incoming::AudioEnd => context.audio.finish_output(),
        Incoming::TurnEnd => {
            context.audio.finish_output();
            let _ = context.events.send(Event::TurnEnd);
        }
        Incoming::Error(message) => {
            context.error(message.replace(&context.key, "[redacted]"));
            return false;
        }
    }
    true
}

async fn handle_ready(
    context: &ConnectionContext,
    socket: &mut Socket,
    state: &mut LiveState,
) -> bool {
    if state.ready {
        return true;
    }
    if state.handle.is_none() && !send_history(context, socket).await {
        return false;
    }
    state.ready = true;
    let _ = context.events.send(Event::Ready);
    true
}

fn queue_output(context: &ConnectionContext, state: &LiveState, bytes: &[u8]) {
    if !state.talking && !state.ignore_old_output {
        context.audio.queue_pcm(bytes);
    }
}

async fn send_history(context: &ConnectionContext, socket: &mut Socket) -> bool {
    for item in protocol::history_messages(context.settings.provider, &context.history) {
        if !send(socket, item).await {
            context.error("Could not restore the selected conversation.");
            return false;
        }
    }
    true
}
