use super::turns::{handle_chunk, handle_command, handle_message, LiveState, MessageResult};
use super::{protocol, Command, ConnectionContext, Event, Provider};
use futures_util::{SinkExt, StreamExt};
use std::{pin::Pin, time::Duration};
use tokio::{net::TcpStream, sync::mpsc};
use tokio_tungstenite::{
    tungstenite::{
        client::IntoClientRequest,
        http::header::{HeaderValue, AUTHORIZATION},
        Message,
    },
    MaybeTlsStream, WebSocketStream,
};

pub(super) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

enum Exit {
    Stop,
    Fatal,
    Disconnected(bool),
}

struct LiveIo<'a> {
    socket: &'a mut Socket,
    commands: &'a mut mpsc::UnboundedReceiver<Command>,
    audio_rx: &'a mut mpsc::Receiver<Vec<u8>>,
}

impl ConnectionContext {
    pub(super) fn error(&self, message: impl Into<String>) {
        let _ = self.events.send(Event::Error(message.into()));
    }
}

pub(super) async fn run_connection(
    context: &ConnectionContext,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
) {
    let url = match protocol::connection_url(&context.settings, &context.key) {
        Ok(url) => url,
        Err(error) => {
            context.error(error);
            return;
        }
    };
    let mut state = LiveState::default();
    let mut reconnects = 0;
    loop {
        let mut socket = match connect(context, &url, commands).await {
            Ok(Some(socket)) => socket,
            Ok(None) | Err(()) => break,
        };
        state.reset_connection();
        if !configure(context, &mut socket, &state).await {
            break;
        }
        let exit = drive(context, &mut socket, commands, &mut audio_rx, &mut state).await;
        context.audio.clear_output();
        if state.ready {
            reconnects = 0;
        }
        let Exit::Disconnected(can_resume) = exit else {
            break;
        };
        if context.settings.provider == Provider::Gemini
            && can_resume
            && state.handle.is_some()
            && reconnects < 2
        {
            reconnects += 1;
            let _ = context.events.send(Event::Status("Reconnecting…".into()));
            continue;
        }
        context
            .error("The conversation disconnected. Check your connection or quota and try again.");
        break;
    }
    context.audio.clear_output();
}

async fn connect(
    context: &ConnectionContext,
    url: &url::Url,
    commands: &mut mpsc::UnboundedReceiver<Command>,
) -> Result<Option<Socket>, ()> {
    let mut request = url.as_str().into_client_request().map_err(|_| {
        context.error("Invalid provider URL.");
    })?;
    if context.settings.provider == Provider::OpenAi {
        let auth = HeaderValue::from_str(&format!("Bearer {}", context.key)).map_err(|_| {
            context.error("Invalid API key.");
        })?;
        request.headers_mut().insert(AUTHORIZATION, auth);
    }
    let attempt = tokio::time::timeout(
        Duration::from_secs(20),
        tokio_tungstenite::connect_async(request),
    );
    tokio::pin!(attempt);
    loop {
        tokio::select! {
            result = &mut attempt => return match result {
                Ok(Ok((socket, _))) => Ok(Some(socket)),
                _ => { context.error("Could not connect. Check the provider URL, key, model access, and network."); Err(()) }
            },
            command = commands.recv() => match command {
                Some(Command::Stop) | None => return Ok(None),
                Some(Command::Mute(_) | Command::Talk(_)) => {},
            },
        }
    }
}

async fn configure(context: &ConnectionContext, socket: &mut Socket, state: &LiveState) -> bool {
    let setup = protocol::setup_with_history(
        &context.settings,
        state.handle.as_deref(),
        !context.history.is_empty(),
    );
    if socket
        .send(Message::Text(setup.to_string().into()))
        .await
        .is_ok()
    {
        return true;
    }
    context.error("Could not configure the live session.");
    false
}

async fn drive(
    context: &ConnectionContext,
    socket: &mut Socket,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    audio_rx: &mut mpsc::Receiver<Vec<u8>>,
    state: &mut LiveState,
) -> Exit {
    let deadline = tokio::time::sleep(Duration::from_secs(20));
    tokio::pin!(deadline);
    let mut io = LiveIo {
        socket,
        commands,
        audio_rx,
    };
    loop {
        if let Some(exit) = select_once(context, &mut io, state, deadline.as_mut()).await {
            return exit;
        }
    }
}

async fn select_once(
    context: &ConnectionContext,
    io: &mut LiveIo<'_>,
    state: &mut LiveState,
    mut deadline: Pin<&mut tokio::time::Sleep>,
) -> Option<Exit> {
    tokio::select! {
        biased;
        _ = &mut deadline, if !state.ready => {
            context.error("The provider did not finish setting up the session. Check model access and try again.");
            Some(Exit::Fatal)
        },
        command = io.commands.recv() => {
            handle_command(context, io.socket, io.audio_rx, state, command).await.then_some(Exit::Stop)
        },
        chunk = io.audio_rx.recv(), if !io.audio_rx.is_closed() || !io.audio_rx.is_empty() => {
            (!forward_chunk(context, io.socket, state, chunk).await).then_some(Exit::Disconnected(false))
        },
        message = io.socket.next() => {
            message_exit(handle_message(context, io.socket, state, message).await)
        }
    }
}

fn message_exit(result: MessageResult) -> Option<Exit> {
    match result {
        MessageResult::Continue => None,
        MessageResult::Fatal => Some(Exit::Fatal),
        MessageResult::Disconnected(can_resume) => Some(Exit::Disconnected(can_resume)),
    }
}

async fn forward_chunk(
    context: &ConnectionContext,
    socket: &mut Socket,
    state: &mut LiveState,
    chunk: Option<Vec<u8>>,
) -> bool {
    match chunk {
        Some(chunk) => handle_chunk(context, socket, state, chunk).await,
        None => true,
    }
}
