use super::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{
    accept_async,
    tungstenite::{http::header::AUTHORIZATION, Message},
};

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
async fn gemini_reconnects_with_the_saved_resume_handle() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        let _ = next_json(&mut socket).await;
        socket
            .send(Message::Text(
                json!({"setupComplete":{}}).to_string().into(),
            ))
            .await
            .unwrap();
        socket
            .send(Message::Text(
                json!({"sessionResumptionUpdate":{"newHandle":"resume-test","resumable":true}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        socket.close(None).await.unwrap();

        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        let setup = next_json(&mut socket).await;
        assert_eq!(setup["setup"]["sessionResumption"]["handle"], "resume-test");
        socket
            .send(Message::Text(
                json!({"setupComplete":{}}).to_string().into(),
            ))
            .await
            .unwrap();
        let _ = socket.next().await;
    });
    let settings = Settings {
        gemini_url: format!("ws://{addr}/live"),
        ..Settings::default()
    };
    let (commands_tx, mut commands_rx) = mpsc::unbounded_channel();
    let (_audio_tx, audio_rx) = mpsc::channel(4);
    let (events_tx, events_rx) = std::sync::mpsc::channel();
    let context = ConnectionContext {
        settings,
        key: "synthetic-key".into(),
        history: Vec::new(),
        audio: AudioControls::new(1.0),
        events: events_tx,
    };
    let client =
        tokio::spawn(async move { run_connection(&context, &mut commands_rx, audio_rx).await });
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut ready_count = 0;
        let mut reconnected = false;
        while ready_count < 2 {
            match events_rx.try_recv() {
                Ok(Event::Ready) => ready_count += 1,
                Ok(Event::Status(status)) if status == "Reconnecting…" => reconnected = true,
                Ok(Event::Error(error)) => panic!("Reconnect failed: {error}"),
                _ => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
        assert!(reconnected);
    })
    .await
    .expect("reconnected session");
    commands_tx.send(Command::Stop).unwrap();
    client.await.unwrap();
    server.await.unwrap();
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
                &ConnectionContext {
                    settings,
                    key: "key".into(),
                    audio: AudioControls::new(1.0),
                    events: events_tx,
                    history: memory,
                },
                &mut commands_rx,
                audio_rx,
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
            &ConnectionContext {
                settings,
                key: "key".into(),
                audio: AudioControls::new(1.0),
                events: events_tx,
                history: Vec::new(),
            },
            &mut commands_rx,
            audio_rx,
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
async fn closed_microphone_channel_does_not_starve_provider_events() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        let _ = next_json(&mut socket).await;
        socket
            .send(Message::Text(
                json!({"setupComplete":{}}).to_string().into(),
            ))
            .await
            .unwrap();
        let _ = socket.next().await;
    });
    let settings = Settings {
        gemini_url: format!("ws://{addr}/live"),
        ..Settings::default()
    };
    let (commands_tx, mut commands_rx) = mpsc::unbounded_channel();
    let (audio_tx, audio_rx) = mpsc::channel(4);
    drop(audio_tx);
    let (events_tx, events_rx) = std::sync::mpsc::channel();
    let context = ConnectionContext {
        settings,
        key: "synthetic-key".into(),
        history: Vec::new(),
        audio: AudioControls::new(1.0),
        events: events_tx,
    };
    let client =
        tokio::spawn(async move { run_connection(&context, &mut commands_rx, audio_rx).await });
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
            &ConnectionContext {
                settings,
                key: "key".into(),
                audio: audio,
                events: events_tx,
                history: Vec::new(),
            },
            &mut commands_rx,
            audio_rx,
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
            &ConnectionContext {
                settings,
                key: "key".into(),
                audio: audio,
                events: events_tx,
                history: Vec::new(),
            },
            &mut commands_rx,
            audio_rx,
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
            &ConnectionContext {
                settings,
                key: "test-key".into(),
                audio: client_audio,
                events: events_tx,
                history: Vec::new(),
            },
            &mut commands_rx,
            audio_rx,
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
        let mut ws = tokio_tungstenite::accept_hdr_async(
            stream,
            |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
             response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                assert_eq!(
                    request.headers().get(AUTHORIZATION).unwrap(),
                    "Bearer test-key"
                );
                Ok(response)
            },
        )
        .await
        .unwrap();
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
            &ConnectionContext {
                settings,
                key: "test-key".into(),
                audio: audio,
                events: events_tx,
                history: Vec::new(),
            },
            &mut commands_rx,
            audio_rx,
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
