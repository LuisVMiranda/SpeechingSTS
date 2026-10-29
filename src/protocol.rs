use crate::config::{InputMode, Provider, Settings};
use crate::history::MemoryMessage;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Speaker {
    User,
    Partner,
}
#[derive(Debug)]
pub enum Event {
    Ready,
    Status(String),
    Error(String),
    Stopped,
    Transcript {
        speaker: Speaker,
        text: String,
        id: Option<String>,
        replace: bool,
    },
    TurnEnd,
}
#[derive(Debug, PartialEq)]
pub enum Incoming {
    Ready,
    Transcript {
        speaker: Speaker,
        text: String,
        id: Option<String>,
        replace: bool,
    },
    Audio(Vec<u8>),
    AudioEnd,
    Interrupt,
    TurnEnd,
    Resume(String),
    Error(String),
}

pub fn connection_url(settings: &Settings, key: &str) -> Result<Url, String> {
    let mut url = Url::parse(settings.endpoint().trim())
        .map_err(|_| "Enter a valid WebSocket URL.".to_string())?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "wss" && !(url.scheme() == "ws" && local) {
        return Err("Use wss:// for a remote provider, or ws:// for a local server.".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Remove credentials from the URL; use the API key field instead.".into());
    }
    match settings.provider {
        Provider::Gemini => {
            url.query_pairs_mut().append_pair("key", key);
        }
        Provider::OpenAi => {
            if !url.query_pairs().any(|(name, _)| name == "model") {
                url.query_pairs_mut().append_pair("model", settings.model());
            }
        }
    }
    Ok(url)
}
#[cfg(test)]
pub fn setup(settings: &Settings, handle: Option<&str>) -> Value {
    setup_with_history(settings, handle, false)
}
pub fn setup_with_history(settings: &Settings, handle: Option<&str>, has_history: bool) -> Value {
    let mut instruction = format!("You are a warm, patient spoken language practice partner. Practice in {}. The learner is at {} level. Keep your answers concise and natural, ask one follow-up question at a time, allow the learner time to speak, and gently correct significant mistakes when useful. Speak in the practice language unless the learner asks for help. {}", settings.language, settings.level.to_lowercase(), pace_instruction(settings.speed));
    if !settings.instructions.trim().is_empty() {
        instruction.push_str("\n\nAdditional instructions from the learner:\n");
        instruction.push_str(settings.instructions.trim());
    }
    let manual = settings.input_mode == InputMode::PushToTalk;
    match settings.provider {
        Provider::Gemini => {
            let mut value = json!({"setup": {"model": format!("models/{}", settings.model().trim_start_matches("models/")),
            "generationConfig": {"responseModalities": ["AUDIO"], "speechConfig": {"voiceConfig": {"prebuiltVoiceConfig": {"voiceName": settings.voice()}}}},
            "systemInstruction": {"parts": [{"text": instruction}]}, "inputAudioTranscription": {}, "outputAudioTranscription": {},
            "realtimeInputConfig": {"automaticActivityDetection": {"disabled": manual}},
            "contextWindowCompression": {"slidingWindow": {}}, "sessionResumption": handle.map_or(json!({}), |h| json!({"handle": h})) }});
            if has_history && handle.is_none() {
                value["setup"]["historyConfig"] = json!({"initialHistoryInClientContent": true});
            }
            value
        }
        Provider::OpenAi => {
            json!({"type": "session.update", "session": {"type": "realtime", "output_modalities": ["audio"], "instructions": instruction,
            "audio": {"input": {"format": {"type": "audio/pcm", "rate": 24000}, "transcription": {"model": "gpt-4o-mini-transcribe"}, "turn_detection": if manual { Value::Null } else { json!({"type": "server_vad"}) }},
                "output": {"format": {"type": "audio/pcm", "rate": 24000}, "voice": settings.voice()}} }})
        }
    }
}

pub fn history_messages(provider: Provider, messages: &[MemoryMessage]) -> Vec<Value> {
    if messages.is_empty() {
        return Vec::new();
    }
    match provider {
        Provider::Gemini => {
            let mut turns: Vec<Value> = Vec::new();
            for message in messages {
                let role = if message.speaker == Speaker::User { "user" } else { "model" };
                if let Some(last) = turns.last_mut().filter(|last| last["role"] == role) {
                    let previous = last["parts"][0]["text"].as_str().unwrap_or("");
                    last["parts"][0]["text"] = json!(format!("{previous}\n{}", message.text));
                } else {
                    turns.push(json!({"role": role, "parts": [{"text": message.text}]}));
                }
            }
            vec![json!({"clientContent": {"turns": turns, "turnComplete": true}})]
        }
        Provider::OpenAi => messages.iter().map(|message| {
            let (role, kind) = if message.speaker == Speaker::User { ("user", "input_text") } else { ("assistant", "text") };
            json!({"type": "conversation.item.create", "item": {"type": "message", "role": role, "content": [{"type": kind, "text": message.text}]}})
        }).collect(),
    }
}
fn pace_instruction(speed: f32) -> String {
    let percent = ((speed - 1.0).abs() * 100.0).round() as u32;
    if speed < 0.785 {
        format!("Speak very slowly and clearly, about {percent}% slower than your usual conversational pace, with natural pauses and an unforced voice.")
    } else if speed < 0.96 {
        format!("Speak a little slower than usual, around {percent}% slower than your normal conversational pace, with clear articulation and natural pauses.")
    } else if speed < 1.08 {
        "Speak at a natural, comfortable conversational pace (about 100% of your usual speed)."
            .into()
    } else if speed < 1.23 {
        format!("Speak somewhat faster than usual, about {percent}% faster than your usual conversational pace, while remaining clear and conversational.")
    } else {
        format!("Speak quickly but clearly, around {percent}% faster than your usual conversational pace, with natural rhythm rather than sounding rushed.")
    }
}
pub fn audio_chunk(provider: Provider, bytes: &[u8]) -> Value {
    let audio = STANDARD.encode(bytes);
    match provider {
        Provider::Gemini => {
            json!({"realtimeInput": {"audio": {"data": audio, "mimeType": "audio/pcm;rate=24000"}}})
        }
        Provider::OpenAi => json!({"type": "input_audio_buffer.append", "audio": audio}),
    }
}
pub fn parse(provider: Provider, payload: &Value) -> Vec<Incoming> {
    let mut result = Vec::new();
    match provider {
        Provider::Gemini => parse_gemini(payload, &mut result),
        Provider::OpenAi => parse_openai(payload, &mut result),
    }
    result
}

fn parse_gemini(payload: &Value, result: &mut Vec<Incoming>) {
    if payload.get("setupComplete").is_some() {
        result.push(Incoming::Ready);
    }
    if let Some(handle) = payload
        .pointer("/sessionResumptionUpdate/newHandle")
        .and_then(Value::as_str)
    {
        if payload
            .pointer("/sessionResumptionUpdate/resumable")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            result.push(Incoming::Resume(handle.into()));
        }
    }
    if let Some(content) = payload.get("serverContent") {
        parse_gemini_content(content, result);
    }
}

fn parse_gemini_content(content: &Value, result: &mut Vec<Incoming>) {
    if content.get("interrupted").and_then(Value::as_bool) == Some(true) {
        result.push(Incoming::Interrupt);
    }
    for (name, speaker) in [
        ("inputTranscription", Speaker::User),
        ("outputTranscription", Speaker::Partner),
    ] {
        if let Some(text) = content
            .pointer(&format!("/{name}/text"))
            .and_then(Value::as_str)
        {
            result.push(Incoming::Transcript {
                speaker,
                text: text.into(),
                id: None,
                replace: false,
            });
        }
    }
    parse_gemini_audio(content, result);
    if content.get("generationComplete").and_then(Value::as_bool) == Some(true) {
        result.push(Incoming::AudioEnd);
    }
    if content.get("turnComplete").and_then(Value::as_bool) == Some(true) {
        result.push(Incoming::TurnEnd);
    }
}

fn parse_gemini_audio(content: &Value, result: &mut Vec<Incoming>) {
    let Some(parts) = content
        .pointer("/modelTurn/parts")
        .and_then(Value::as_array)
    else {
        return;
    };
    for part in parts {
        if let Some(data) = part
            .pointer("/inlineData/data")
            .and_then(Value::as_str)
            .and_then(|s| STANDARD.decode(s).ok())
        {
            result.push(Incoming::Audio(data));
        }
    }
}

fn parse_openai(payload: &Value, result: &mut Vec<Incoming>) {
    let kind = payload.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "session.updated" | "input_audio_buffer.speech_started" => {
            result.push(openai_signal(kind));
        }
        "conversation.item.input_audio_transcription.delta" => {
            add_text(result, payload, "delta", Speaker::User, false)
        }
        "conversation.item.input_audio_transcription.completed" => {
            add_text(result, payload, "transcript", Speaker::User, true)
        }
        "response.output_audio_transcript.delta" | "response.audio_transcript.delta" => {
            add_text(result, payload, "delta", Speaker::Partner, false)
        }
        "response.output_audio_transcript.done" | "response.audio_transcript.done" => {
            add_text(result, payload, "transcript", Speaker::Partner, true)
        }
        "response.output_audio.delta" | "response.audio.delta" => {
            parse_openai_audio(payload, result);
        }
        "response.output_audio.done" | "response.audio.done" => result.push(Incoming::AudioEnd),
        "response.done" => result.push(Incoming::TurnEnd),
        "error" => result.push(Incoming::Error(
            payload
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("The provider reported an error.")
                .chars()
                .take(180)
                .collect(),
        )),
        _ => {}
    }
}

fn openai_signal(kind: &str) -> Incoming {
    if kind == "session.updated" {
        Incoming::Ready
    } else {
        Incoming::Interrupt
    }
}

fn parse_openai_audio(payload: &Value, result: &mut Vec<Incoming>) {
    if let Some(data) = payload
        .get("delta")
        .and_then(Value::as_str)
        .and_then(|s| STANDARD.decode(s).ok())
    {
        result.push(Incoming::Audio(data));
    }
}

fn add_text(
    result: &mut Vec<Incoming>,
    payload: &Value,
    field: &str,
    speaker: Speaker,
    replace: bool,
) {
    let id = payload
        .get("item_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(text) = payload.get(field).and_then(Value::as_str) {
        result.push(Incoming::Transcript {
            speaker,
            text: text.into(),
            id,
            replace,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gemini_setup_requests_voice_and_both_transcripts() {
        let setup = setup(&Settings::default(), None);
        assert_eq!(
            setup["setup"]["generationConfig"]["speechConfig"]["voiceConfig"]
                ["prebuiltVoiceConfig"]["voiceName"],
            "Aoede"
        );
        assert!(setup["setup"].get("inputAudioTranscription").is_some());
        assert!(setup["setup"].get("outputAudioTranscription").is_some());
    }
    #[test]
    fn interface_language_does_not_change_the_models_practice_language() {
        let mut settings = Settings::default();
        settings.language = "Spanish".into();
        settings.ui_language = crate::i18n::UiLanguage::PortugueseBrazil;
        let instruction = setup(&settings, None)["setup"]["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(instruction.contains("Practice in Spanish."));
        assert!(!instruction.contains("Espanhol"));
    }
    #[test]
    fn pacing_is_sent_as_a_speaking_instruction_for_both_providers() {
        let mut settings = Settings::default();
        settings.speed = 0.75;
        let gemini = setup(&settings, None);
        assert!(gemini["setup"]["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Speak very slowly"));
        settings.provider = Provider::OpenAi;
        let openai = setup(&settings, None);
        assert!(openai["session"]["instructions"]
            .as_str()
            .unwrap()
            .contains("Speak very slowly"));
        assert!(openai["session"]["audio"]["output"].get("speed").is_none());
        settings.speed = 0.82;
        let instruction = setup(&settings, None)["session"]["instructions"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(instruction.contains("18% slower"));
    }
    #[test]
    fn openai_audio_and_transcripts_are_parsed() {
        let events = parse(
            Provider::OpenAi,
            &json!({"type":"conversation.item.input_audio_transcription.completed", "item_id":"one", "transcript":"Hello"}),
        );
        assert_eq!(
            events,
            vec![Incoming::Transcript {
                speaker: Speaker::User,
                text: "Hello".into(),
                id: Some("one".into()),
                replace: true
            }]
        );
        assert!(matches!(
            parse(
                Provider::OpenAi,
                &json!({"type":"response.output_audio.delta", "delta":"AAE="})
            )[0],
            Incoming::Audio(_)
        ));
    }
    #[test]
    fn output_completion_flushes_the_audio_processor() {
        assert_eq!(
            parse(
                Provider::OpenAi,
                &json!({"type":"response.output_audio.done"})
            ),
            vec![Incoming::AudioEnd]
        );
        assert_eq!(
            parse(
                Provider::Gemini,
                &json!({"serverContent":{"generationComplete":true}})
            ),
            vec![Incoming::AudioEnd]
        );
    }
    #[test]
    fn custom_instructions_and_manual_turn_detection_reach_both_providers() {
        let mut settings = Settings::default();
        settings.instructions = "Ask me about astronomy.".into();
        settings.input_mode = InputMode::PushToTalk;
        let gemini = setup(&settings, None);
        assert!(gemini["setup"]["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Ask me about astronomy."));
        assert_eq!(
            gemini["setup"]["realtimeInputConfig"]["automaticActivityDetection"]["disabled"],
            true
        );
        settings.provider = Provider::OpenAi;
        let openai = setup(&settings, None);
        assert!(openai["session"]["instructions"]
            .as_str()
            .unwrap()
            .contains("Ask me about astronomy."));
        assert!(openai["session"]["audio"]["input"]["turn_detection"].is_null());
    }
    #[test]
    fn remote_cleartext_connections_are_rejected() {
        let mut s = Settings::default();
        s.gemini_url = "ws://example.com/live".into();
        assert!(connection_url(&s, "key").is_err());
        s.gemini_url = "ws://127.0.0.1:1234/live".into();
        assert!(connection_url(&s, "key").is_ok());
    }
}
