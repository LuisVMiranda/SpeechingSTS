use crate::{
    audio::{AudioControls, AudioStreams},
    config::{InputMode, Provider, Settings},
    history::MemoryMessage,
    protocol::{self, Event, Incoming},
};
use futures_util::{SinkExt, StreamExt};
use std::{
    sync::{atomic::Ordering, mpsc::Sender},
    thread,
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    http::header::{HeaderValue, AUTHORIZATION},
    Message,
};

pub enum Command {
    Stop,
    Mute(bool),
    Talk(bool),
}
pub struct Session {
    commands: mpsc::UnboundedSender<Command>,
    pub audio: AudioControls,
}
impl Session {
    pub fn start(
        settings: Settings,
        key: String,
        history: Vec<MemoryMessage>,
        events: Sender<Event>,
    ) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let audio = AudioControls::new(settings.speed).configure_input(
            settings.input_mode == InputMode::PushToTalk,
            settings
                .noise_gate_enabled
                .then_some(settings.noise_gate_db),
        );
        let thread_audio = audio.clone();
        thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(_) => {
                    let _ = events.send(Event::Error("Could not start the audio engine.".into()));
                    return;
                }
            };
            runtime.block_on(run(settings, key, history, thread_audio, receiver, events));
        });
        Self { commands, audio }
    }
    pub fn mute(&self, value: bool) {
        if value {
            self.talk(false);
        }
        self.audio.muted.store(value, Ordering::Relaxed);
        let _ = self.commands.send(Command::Mute(value));
    }
    pub fn talk(&self, value: bool) {
        self.audio.talking.store(value, Ordering::Relaxed);
        let _ = self.commands.send(Command::Talk(value));
    }
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn run(
    settings: Settings,
    key: String,
    history: Vec<MemoryMessage>,
    audio: AudioControls,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: Sender<Event>,
) {
    let (audio_tx, audio_rx) = mpsc::channel(32);
    let _streams = match AudioStreams::start(audio.clone(), audio_tx) {
        Ok(streams) => streams,
        Err(error) => {
            let _ = events.send(Event::Error(error));
            return;
        }
    };
    run_connection(
        settings,
        key,
        audio,
        &mut commands,
        audio_rx,
        &events,
        &history,
    )
    .await;
    let _ = events.send(Event::Stopped);
}

async fn run_connection(
    settings: Settings,
    key: String,
    audio: AudioControls,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
    events: &Sender<Event>,
    history: &[MemoryMessage],
) {
    let url = match protocol::connection_url(&settings, &key) {
        Ok(url) => url,
        Err(error) => {
            let _ = events.send(Event::Error(error));
            return;
        }
    };
    let mut handle: Option<String> = None;
    let mut reconnects = 0;
    'sessions: loop {
        let mut request = match url.as_str().into_client_request() {
            Ok(request) => request,
            Err(_) => {
                let _ = events.send(Event::Error("Invalid provider URL.".into()));
                break;
            }
        };
        if settings.provider == Provider::OpenAi {
            let Ok(auth) = HeaderValue::from_str(&format!("Bearer {key}")) else {
                let _ = events.send(Event::Error("Invalid API key.".into()));
                break;
            };
            request.headers_mut().insert(AUTHORIZATION, auth);
        }
        let connection_attempt = tokio::time::timeout(
            Duration::from_secs(20),
            tokio_tungstenite::connect_async(request),
        );
        tokio::pin!(connection_attempt);
        let connection = loop {
            tokio::select! {
                result = &mut connection_attempt => break result,
                command = commands.recv() => match command {
                    Some(Command::Stop) | None => break 'sessions,
                    Some(Command::Mute(_) | Command::Talk(_)) => {},
                },
            }
        };
        let (mut socket, _) = match connection {
            Ok(Ok(socket)) => socket,
            _ => {
                let _ = events.send(Event::Error(
                    "Could not connect. Check the provider URL, key, model access, and network."
                        .into(),
                ));
                break;
            }
        };
        if socket
            .send(Message::Text(
                protocol::setup_with_history(&settings, handle.as_deref(), !history.is_empty())
                    .to_string()
                    .into(),
            ))
            .await
            .is_err()
        {
            let _ = events.send(Event::Error("Could not configure the live session.".into()));
            break;
        }
        let mut ready = false;
        let mut can_resume = false;
        let mut talking = false;
        let mut talk_bytes = 0usize;
        let mut response_active = false;
        let mut ignore_old_output = false;
        let setup_deadline = tokio::time::sleep(Duration::from_secs(20));
        tokio::pin!(setup_deadline);
        loop {
            tokio::select! {
                biased;
                _ = &mut setup_deadline, if !ready => { let _ = events.send(Event::Error("The provider did not finish setting up the session. Check model access and try again.".into())); break 'sessions; },
                command = commands.recv() => match command {
                    Some(Command::Mute(true)) if ready && settings.provider == Provider::Gemini && settings.input_mode == InputMode::VoiceActivated => {
                        let _ = socket.send(Message::Text(serde_json::json!({"realtimeInput":{"audioStreamEnd":true}}).to_string().into())).await;
                    }
                    Some(Command::Mute(_)) => {},
                    Some(Command::Talk(true)) if ready && settings.input_mode == InputMode::PushToTalk && !talking => {
                        audio.clear_output();
                        talk_bytes = 0;
                        if settings.provider == Provider::Gemini {
                            let _ = socket.send(Message::Text(serde_json::json!({"realtimeInput":{"activityStart":{}}}).to_string().into())).await;
                        } else {
                            if response_active {
                                ignore_old_output = true;
                                let _ = socket.send(Message::Text(serde_json::json!({"type":"response.cancel"}).to_string().into())).await;
                            }
                            let _ = socket.send(Message::Text(serde_json::json!({"type":"input_audio_buffer.clear"}).to_string().into())).await;
                        }
                        talking = true;
                    }
                    Some(Command::Talk(false)) if ready && talking => {
                        while let Ok(chunk) = audio_rx.try_recv() {
                            talk_bytes += chunk.len();
                            let _ = socket.send(Message::Text(protocol::audio_chunk(settings.provider, &chunk).to_string().into())).await;
                        }
                        talking = false;
                        if settings.provider == Provider::Gemini {
                            let _ = socket.send(Message::Text(serde_json::json!({"realtimeInput":{"activityEnd":{}}}).to_string().into())).await;
                        } else if talk_bytes >= 4_800 {
                            let _ = socket.send(Message::Text(serde_json::json!({"type":"input_audio_buffer.commit"}).to_string().into())).await;
                            let _ = socket.send(Message::Text(serde_json::json!({"type":"response.create"}).to_string().into())).await;
                        } else {
                            let _ = socket.send(Message::Text(serde_json::json!({"type":"input_audio_buffer.clear"}).to_string().into())).await;
                        }
                    }
                    Some(Command::Talk(_)) => {},
                    _ => { let _ = socket.close(None).await; break 'sessions; }
                },
                chunk = audio_rx.recv() => {
                    if let Some(chunk) = chunk {
                        let allowed = if settings.input_mode == InputMode::PushToTalk { talking } else { !audio.muted.load(Ordering::Relaxed) };
                        if ready && allowed {
                            if talking { talk_bytes += chunk.len(); }
                            if socket.send(Message::Text(protocol::audio_chunk(settings.provider, &chunk).to_string().into())).await.is_err() { break; }
                        }
                    }
                },
                message = socket.next() => {
                    let text = match message {
                        Some(Ok(Message::Text(text))) => text.to_string(),
                        Some(Ok(Message::Binary(bytes))) => String::from_utf8_lossy(&bytes).into_owned(),
                        Some(Ok(Message::Close(frame))) => { can_resume = frame.as_ref().map(|f| u16::from(f.code) != 1008).unwrap_or(true); break; },
                        Some(Ok(_)) => continue,
                        _ => { can_resume = true; break; }
                    };
                    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
                    if settings.provider == Provider::OpenAi {
                        match payload.get("type").and_then(serde_json::Value::as_str) {
                            Some("response.created") => { response_active = true; ignore_old_output = false; },
                            Some("response.done") => { response_active = false; ignore_old_output = false; },
                            _ => {}
                        }
                    }
                    for event in protocol::parse(settings.provider, &payload) {
                        match event {
                            Incoming::Ready => {
                                if !ready {
                                    if handle.is_none() {
                                        for item in protocol::history_messages(settings.provider, history) {
                                            if socket.send(Message::Text(item.to_string().into())).await.is_err() {
                                                let _ = events.send(Event::Error("Could not restore the selected conversation.".into()));
                                                break 'sessions;
                                            }
                                        }
                                    }
                                    ready = true;
                                    reconnects = 0;
                                    let _ = events.send(Event::Ready);
                                }
                            }
                            Incoming::Audio(bytes) => { if !talking && !ignore_old_output { audio.queue_pcm(&bytes); } },
                            Incoming::Interrupt => { audio.clear_output(); let _ = events.send(Event::TurnEnd); },
                            Incoming::Resume(value) => handle = Some(value),
                            Incoming::Transcript { speaker, text, id, replace } => { let _ = events.send(Event::Transcript { speaker, text, id, replace }); }
                            Incoming::AudioEnd => audio.finish_output(),
                            Incoming::TurnEnd => { audio.finish_output(); let _ = events.send(Event::TurnEnd); }
                            Incoming::Error(message) => { let _ = events.send(Event::Error(message.replace(&key, "[redacted]"))); break 'sessions; }
                        }
                    }
                }
            }
        }
        audio.clear_output();
        if settings.provider == Provider::Gemini && can_resume && handle.is_some() && reconnects < 2
        {
            reconnects += 1;
            let _ = events.send(Event::Status("Reconnecting…".into()));
            continue;
        }
        let _ = events.send(Event::Error(
            "The conversation disconnected. Check your connection or quota and try again.".into(),
        ));
        break;
    }
    audio.clear_output();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    async fn wait_for_ready(receiver: &std::sync::mpsc::Receiver<Event>) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                match receiver.try_recv() {
                    Ok(Event::Ready) => break,
                    Ok(Event::Error(message)) => panic!("Session failed: {message}"),
                    _ => tokio::time::sleep(Duration::from_millis(5)).await,
                }
            }
        })
        .await
        .expect("session ready");
    }

    async fn next_json(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    ) -> Value {
        let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        serde_json::from_str(message.to_text().unwrap()).unwrap()
    }

    #[tokio::test]
    async fn restored_history_precedes_audio_without_requesting_a_response() {
        use crate::protocol::Speaker;
        for provider in [Provider::Gemini, Provider::OpenAi] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (received_tx, received_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws = accept_async(stream).await.unwrap();
                let setup = next_json(&mut ws).await;
                if provider == Provider::Gemini {
                    assert_eq!(
                        setup["setup"]["historyConfig"]["initialHistoryInClientContent"],
                        true
                    );
                    ws.send(Message::Text(
                        json!({"setupComplete": {}}).to_string().into(),
                    ))
                    .await
                    .unwrap();
                    let history = next_json(&mut ws).await;
                    assert_eq!(history["clientContent"]["turnComplete"], true);
                    assert_eq!(history["clientContent"]["turns"][0]["role"], "user");
                    assert_eq!(
                        history["clientContent"]["turns"][0]["parts"][0]["text"],
                        "Prior question"
                    );
                    assert_eq!(history["clientContent"]["turns"][1]["role"], "model");
                } else {
                    ws.send(Message::Text(
                        json!({"type": "session.updated"}).to_string().into(),
                    ))
                    .await
                    .unwrap();
                    let user = next_json(&mut ws).await;
                    let partner = next_json(&mut ws).await;
                    assert_eq!(user["type"], "conversation.item.create");
                    assert_eq!(user["item"]["content"][0]["text"], "Prior question");
                    assert_eq!(partner["item"]["role"], "assistant");
                    assert_eq!(partner["item"]["content"][0]["type"], "text");
                }
                let _ = received_tx.send(());
                let next = next_json(&mut ws).await;
                if provider == Provider::Gemini {
                    assert!(next["realtimeInput"].get("audio").is_some());
                } else {
                    assert_eq!(next["type"], "input_audio_buffer.append");
                }
                let _ = ws.next().await;
            });
            let mut settings = Settings::default();
            settings.provider = provider;
            if provider == Provider::Gemini {
                settings.gemini_url = format!("ws://{addr}/live");
            } else {
                settings.openai_url = format!("ws://{addr}/live");
            }
            let (commands_tx, mut commands_rx) = mpsc::unbounded_channel();
            let (audio_tx, audio_rx) = mpsc::channel(4);
            let (events_tx, events_rx) = std::sync::mpsc::channel();
            let client = tokio::spawn(async move {
                let memory = vec![
                    MemoryMessage {
                        speaker: Speaker::User,
                        text: "Prior question".into(),
                    },
                    MemoryMessage {
                        speaker: Speaker::Partner,
                        text: "Prior answer".into(),
                    },
                ];
                run_connection(
                    settings,
                    "key".into(),
                    AudioControls::new(1.0),
                    &mut commands_rx,
                    audio_rx,
                    &events_tx,
                    &memory,
                )
                .await;
            });
            wait_for_ready(&events_rx).await;
            received_rx.await.unwrap();
            audio_tx.send(vec![0, 1]).await.unwrap();
            tokio::time::sleep(Duration::from_millis(40)).await;
            commands_tx.send(Command::Stop).unwrap();
            client.await.unwrap();
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn talk_and_mute_updates_do_not_cancel_an_inflight_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _ = accepted_tx.send(());
            resume_rx.await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let _ = next_json(&mut ws).await;
            ws.send(Message::Text(
                json!({"setupComplete":{}}).to_string().into(),
            ))
            .await
            .unwrap();
            let _ = ws.next().await;
        });
        let mut settings = Settings::default();
        settings.gemini_url = format!("ws://{addr}/live");
        let (commands_tx, mut commands_rx) = mpsc::unbounded_channel();
        let (_audio_tx, audio_rx) = mpsc::channel(4);
        let (events_tx, events_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move {
            run_connection(
                settings,
                "key".into(),
                AudioControls::new(1.0),
                &mut commands_rx,
                audio_rx,
                &events_tx,
                &[],
            )
            .await
        });
        accepted_rx.await.unwrap();
        commands_tx.send(Command::Talk(false)).unwrap();
        commands_tx.send(Command::Mute(false)).unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        resume_tx.send(()).unwrap();
        wait_for_ready(&events_rx).await;
        commands_tx.send(Command::Stop).unwrap();
        client.await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn gemini_push_to_talk_sends_start_audio_and_end_in_order() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let setup = next_json(&mut ws).await;
            assert_eq!(
                setup["setup"]["realtimeInputConfig"]["automaticActivityDetection"]["disabled"],
                true
            );
            ws.send(Message::Text(
                json!({"setupComplete":{}}).to_string().into(),
            ))
            .await
            .unwrap();
            assert!(next_json(&mut ws).await["realtimeInput"]
                .get("activityStart")
                .is_some());
            assert!(next_json(&mut ws).await["realtimeInput"]
                .get("audio")
                .is_some());
            assert!(next_json(&mut ws).await["realtimeInput"]
                .get("activityEnd")
                .is_some());
            let _ = done_tx.send(());
            let _ = ws.next().await;
        });
        let mut settings = Settings::default();
        settings.input_mode = InputMode::PushToTalk;
        settings.gemini_url = format!("ws://{addr}/live");
        let audio = AudioControls::new(1.0).configure_input(true, None);
        let (commands_tx, mut commands_rx) = mpsc::unbounded_channel();
        let (audio_tx, audio_rx) = mpsc::channel(8);
        let (events_tx, events_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move {
            run_connection(
                settings,
                "key".into(),
                audio,
                &mut commands_rx,
                audio_rx,
                &events_tx,
                &[],
            )
            .await
        });
        wait_for_ready(&events_rx).await;
        commands_tx.send(Command::Talk(true)).unwrap();
        audio_tx.send(vec![0; 1_920]).await.unwrap();
        commands_tx.send(Command::Talk(false)).unwrap();
        tokio::time::timeout(Duration::from_secs(3), done_rx)
            .await
            .unwrap()
            .unwrap();
        commands_tx.send(Command::Stop).unwrap();
        client.await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn openai_push_to_talk_commits_audio_then_requests_a_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let setup = next_json(&mut ws).await;
            assert!(setup["session"]["audio"]["input"]["turn_detection"].is_null());
            ws.send(Message::Text(
                json!({"type":"session.updated"}).to_string().into(),
            ))
            .await
            .unwrap();
            assert_eq!(next_json(&mut ws).await["type"], "input_audio_buffer.clear");
            for _ in 0..3 {
                assert_eq!(
                    next_json(&mut ws).await["type"],
                    "input_audio_buffer.append"
                );
            }
            assert_eq!(
                next_json(&mut ws).await["type"],
                "input_audio_buffer.commit"
            );
            assert_eq!(next_json(&mut ws).await["type"], "response.create");
            let _ = done_tx.send(());
            let _ = ws.next().await;
        });
        let mut settings = Settings::default();
        settings.provider = Provider::OpenAi;
        settings.input_mode = InputMode::PushToTalk;
        settings.openai_url = format!("ws://{addr}/live");
        let audio = AudioControls::new(1.0).configure_input(true, None);
        let (commands_tx, mut commands_rx) = mpsc::unbounded_channel();
        let (audio_tx, audio_rx) = mpsc::channel(8);
        let (events_tx, events_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move {
            run_connection(
                settings,
                "key".into(),
                audio,
                &mut commands_rx,
                audio_rx,
                &events_tx,
                &[],
            )
            .await
        });
        wait_for_ready(&events_rx).await;
        commands_tx.send(Command::Talk(true)).unwrap();
        for _ in 0..3 {
            audio_tx.send(vec![0; 1_920]).await.unwrap();
        }
        commands_tx.send(Command::Talk(false)).unwrap();
        tokio::time::timeout(Duration::from_secs(3), done_rx)
            .await
            .unwrap()
            .unwrap();
        commands_tx.send(Command::Stop).unwrap();
        client.await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn gemini_session_streams_audio_and_transcripts_through_a_mock_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let setup: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(setup["setup"]["model"], "models/gemini-3.8-live");
            ws.send(Message::Text(
                json!({"setupComplete": {}}).to_string().into(),
            ))
            .await
            .unwrap();
            let audio: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(
                audio["realtimeInput"]["audio"]["mimeType"],
                "audio/pcm;rate=24000"
            );
            ws.send(Message::Text(json!({"serverContent":{"inputTranscription":{"text":"Hello"},"outputTranscription":{"text":"Hi"},"modelTurn":{"parts":[{"inlineData":{"data":"AAE=","mimeType":"audio/pcm;rate=24000"}}]},"turnComplete":true}}).to_string().into())).await.unwrap();
            let _ = ws.next().await;
        });
        let mut settings = Settings::default();
        settings.gemini_url = format!("ws://{addr}/live");
        let audio = AudioControls::new(1.0);
        let (commands_tx, mut commands_rx) = mpsc::unbounded_channel();
        let (audio_tx, audio_rx) = mpsc::channel(4);
        let (events_tx, events_rx) = std::sync::mpsc::channel();
        let client_audio = audio.clone();
        let client = tokio::spawn(async move {
            run_connection(
                settings,
                "test-key".into(),
                client_audio,
                &mut commands_rx,
                audio_rx,
                &events_tx,
                &[],
            )
            .await
        });
        // The client only forwards microphone data after receiving setupComplete.
        tokio::time::sleep(Duration::from_millis(80)).await;
        audio_tx.send(vec![0, 1]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        let received: Vec<_> = events_rx.try_iter().collect();
        assert!(received.iter().any(|e| matches!(e, Event::Ready)));
        assert_eq!(
            received
                .iter()
                .filter(|e| matches!(e, Event::Transcript { .. }))
                .count(),
            2
        );
        assert_eq!(audio.output.lock().unwrap().len(), 1);
        commands_tx.send(Command::Stop).unwrap();
        client.await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn openai_session_authenticates_and_streams_through_a_mock_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_hdr_async(stream, |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                assert_eq!(request.headers().get(AUTHORIZATION).unwrap(), "Bearer test-key");
                Ok(response)
            }).await.unwrap();
            let setup: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(setup["type"], "session.update");
            assert_eq!(setup["session"]["audio"]["output"]["voice"], "marin");
            ws.send(Message::Text(
                json!({"type":"session.updated"}).to_string().into(),
            ))
            .await
            .unwrap();
            let audio: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(audio["type"], "input_audio_buffer.append");
            ws.send(Message::Text(json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"user-1","transcript":"Hello"}).to_string().into())).await.unwrap();
            ws.send(Message::Text(json!({"type":"response.output_audio_transcript.delta","item_id":"reply-1","delta":"Hi"}).to_string().into())).await.unwrap();
            let _ = ws.next().await;
        });
        let mut settings = Settings::default();
        settings.provider = Provider::OpenAi;
        settings.openai_url = format!("ws://{addr}/live");
        let audio = AudioControls::new(1.0);
        let (commands_tx, mut commands_rx) = mpsc::unbounded_channel();
        let (audio_tx, audio_rx) = mpsc::channel(4);
        let (events_tx, events_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move {
            run_connection(
                settings,
                "test-key".into(),
                audio,
                &mut commands_rx,
                audio_rx,
                &events_tx,
                &[],
            )
            .await
        });
        tokio::time::sleep(Duration::from_millis(80)).await;
        audio_tx.send(vec![0, 1]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        let received: Vec<_> = events_rx.try_iter().collect();
        assert!(received.iter().any(|e| matches!(e, Event::Ready)));
        assert_eq!(
            received
                .iter()
                .filter(|e| matches!(e, Event::Transcript { .. }))
                .count(),
            2
        );
        commands_tx.send(Command::Stop).unwrap();
        client.await.unwrap();
        server.await.unwrap();
    }
}
