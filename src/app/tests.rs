use super::*;
#[cfg(windows)]
fn request_exit(app: &mut App, ctx: &egui::Context) -> egui::FullOutput {
    let mut input = egui::RawInput::default();
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    let mut frame = eframe::Frame::_new_kittest();
    ctx.run_ui(input, |ui| eframe::App::logic(app, ui.ctx(), &mut frame))
}

#[cfg(windows)]
#[test]
fn exiting_app_saves_a_pending_key_or_cancels_exit_on_failure() {
    let namespace = format!(
        "Speeching/test/app-exit/{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let store = credentials::Store::isolated(&namespace);
    let context = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = App::with_settings_and_store(&context, Settings::default(), store.clone());
    app.gemini_key = "synthetic-exit-key".into();

    let mut success = request_exit(&mut app, &context.egui_ctx);
    let saved_key = store.load(Provider::Gemini);
    app.gemini_key = "x".repeat(2561);
    let mut failure = request_exit(&mut app, &context.egui_ctx);
    let retained_key = store.load(Provider::Gemini);
    store.forget(Provider::Gemini).unwrap();

    let cancelled = |output: &egui::FullOutput| {
        output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&egui::ViewportCommand::CancelClose)
    };
    let success_cancelled = cancelled(&success);
    let failure_cancelled = cancelled(&failure);
    success.textures_delta.clear();
    failure.textures_delta.clear();
    assert!(!success_cancelled);
    assert!(failure_cancelled);
    assert_eq!(saved_key.unwrap().as_deref(), Some("synthetic-exit-key"));
    assert_eq!(retained_key.unwrap().as_deref(), Some("synthetic-exit-key"));
    assert_eq!(app.gemini_key.len(), 2561);
    assert!(app.settings_open);
}

#[cfg(windows)]
#[test]
fn closing_settings_saves_pending_keys_for_both_providers() {
    let namespace = format!(
        "Speeching/test/settings-close/{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let store = credentials::Store::isolated(&namespace);
    let context = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = App::with_settings_and_store(&context, Settings::default(), store.clone());
    app.settings_open = true;
    app.gemini_key = "synthetic-gemini-key".into();
    app.openai_key = "synthetic-openai-key".into();

    app.close_settings();
    let gemini = store.load(Provider::Gemini);
    let openai = store.load(Provider::OpenAi);
    let mut restart_settings = Settings::default();
    restart_settings.gemini_model.clear();
    let mut restarted = App::with_settings_and_store(&context, restart_settings, store.clone());
    let saved_flags = (restarted.gemini_key_saved, restarted.openai_key_saved);
    let diagnostic = format!(
        "settings_open={}, save_feedback={:?}, reload_feedback={:?}, reads={:?}/{:?}",
        app.settings_open,
        app.key_feedback,
        restarted.key_feedback,
        gemini.as_ref().map(|key| key.is_some()),
        openai.as_ref().map(|key| key.is_some())
    );
    let drafts_empty = (
        restarted.gemini_key.is_empty(),
        restarted.openai_key.is_empty(),
    );
    restarted.start();
    let start_status = restarted.status.clone();
    store.forget(Provider::Gemini).unwrap();
    store.forget(Provider::OpenAi).unwrap();

    assert_eq!(saved_flags, (true, true), "{diagnostic}");
    assert_eq!(drafts_empty, (true, true));
    assert_eq!(
        start_status,
        "Choose a language, model, and voice in settings."
    );
    assert_eq!(gemini.unwrap().as_deref(), Some("synthetic-gemini-key"));
    assert_eq!(openai.unwrap().as_deref(), Some("synthetic-openai-key"));
    assert!(!app.settings_open);
}
#[cfg(windows)]
#[test]
fn failed_key_save_keeps_settings_and_draft_open() {
    let namespace = format!("Speeching/test/save-failure/{}", std::process::id());
    let store = credentials::Store::isolated(&namespace);
    store
        .save(Provider::Gemini, "synthetic-original-key")
        .unwrap();
    let context = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = App::with_settings_and_store(&context, Settings::default(), store.clone());
    app.settings_open = true;
    app.gemini_key = "x".repeat(2561);

    app.close_settings();
    let existing_key = store.load(Provider::Gemini);
    store.forget(Provider::Gemini).unwrap();

    assert!(app.settings_open);
    assert_eq!(
        existing_key.unwrap().as_deref(),
        Some("synthetic-original-key")
    );
    assert_eq!(app.gemini_key.len(), 2561);
    assert_eq!(app.key_feedback, "API key is too long to save.");
    assert_eq!(app.settings_section, SettingsSection::Provider);
}
#[test]
fn speaking_indicator_holds_through_brief_pauses() {
    let start = Instant::now();
    let mut indicator = SpeechIndicator::default();
    indicator.update(start, 0.05);
    assert!(!indicator.speaking);
    indicator.update(start + Duration::from_millis(120), 0.05);
    assert!(indicator.speaking);
    indicator.update(start + Duration::from_millis(500), 0.0);
    assert!(indicator.speaking);
    indicator.update(start + Duration::from_millis(900), 0.0);
    assert!(!indicator.speaking);
    indicator.update(start + Duration::from_millis(920), 0.05);
    indicator.update(start + Duration::from_millis(950), 0.0);
    assert!(!indicator.speaking);
}
#[test]
fn transcript_keeps_full_history_and_updates_completed_items() {
    let mut t = Transcript::default();
    let start = Instant::now();
    t.begin_session("Aoede".into(), start);
    t.append(Speaker::User, "Hel".into(), Some("a".into()), false);
    t.append(Speaker::User, "lo".into(), Some("a".into()), false);
    t.append(Speaker::User, "Hello!".into(), Some("a".into()), true);
    t.append(Speaker::Partner, "Hi.".into(), None, false);
    assert_eq!(t.lines.len(), 2);
    t.finish_session(start + Duration::from_secs(71));
    assert_eq!(
        t.text(UiLanguage::English),
        "Conversation · AI Partner · 01:11\n\nYou: Hello!\n\nAI Partner: Hi."
    );
    assert_eq!(
        t.text(UiLanguage::Spanish),
        "Conversación · Compañero de IA · 01:11\n\nTú: Hello!\n\nCompañero de IA: Hi."
    );
    assert_eq!(
        t.text(UiLanguage::PortugueseBrazil),
        "Conversa · Parceiro de IA · 01:11\n\nVocê: Hello!\n\nParceiro de IA: Hi."
    );
    t.clear();
    assert!(t.lines.is_empty());
}

#[test]
fn partner_introduction_updates_labels_and_restores_from_saved_messages() {
    let now = Instant::now();
    let mut transcript = Transcript::default();
    transcript.begin_session("Aoede".into(), now);
    transcript.append(Speaker::User, "My name is Maria.".into(), None, false);
    transcript.append(Speaker::Partner, "Hello! ".into(), None, false);
    assert_eq!(transcript.partner_name(UiLanguage::English), "AI Partner");
    transcript.append(Speaker::Partner, "My name is An".into(), None, false);
    assert_eq!(transcript.partner_name(UiLanguage::English), "AI Partner");
    transcript.append(Speaker::Partner, "a.".into(), None, false);
    assert_eq!(transcript.partner_name(UiLanguage::English), "Ana");
    assert_eq!(
        transcript.speaker_name(&transcript.lines[1], UiLanguage::English),
        "Ana"
    );
    assert_eq!(
        transcript.speaker_name(&transcript.lines[0], UiLanguage::English),
        "You"
    );
    transcript.finish_session(now + Duration::from_secs(2));

    let mut record = ChatRecord::new("English".into());
    record.sessions = transcript.saved_sessions(now + Duration::from_secs(2));
    let encoded = serde_json::to_string(&record).unwrap();
    let restored: ChatRecord = serde_json::from_str(&encoded).unwrap();
    let mut transcript = Transcript::from_record(&restored, now);
    assert_eq!(transcript.partner_name(UiLanguage::English), "Ana");
    transcript.begin_session("Kore".into(), now);
    assert_eq!(transcript.partner_name(UiLanguage::English), "Ana");
}

#[cfg(windows)]
#[test]
fn automatic_log_opening_only_occurs_when_an_active_session_ends() {
    let context = eframe::CreationContext::_new_kittest(egui::Context::default());
    for enabled in [false, true] {
        for event in [Event::Stopped, Event::Error("Connection lost".into())] {
            let mut settings = Settings::default();
            settings.open_log_after_conversation = enabled;
            let store = credentials::Store::empty_for_test();
            let mut app = App::with_settings_and_store(&context, settings, store);
            app.transcript.begin_session("Aoede".into(), Instant::now());
            let (sender, receiver) = mpsc::channel();
            app.receiver = Some(receiver);
            sender.send(event).unwrap();
            app.update_events();
            assert_eq!(app.log_open, enabled);
            app.log_open = false;
            app.finish_conversation(Instant::now());
            assert!(!app.log_open);
        }
    }
}

#[cfg(windows)]
#[test]
fn settings_and_log_render_the_preference_and_partner_name() {
    fn collect_text(shape: &egui::Shape, output: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => output.push(text.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, output);
                }
            }
            _ => {}
        }
    }
    let context = eframe::CreationContext::_new_kittest(egui::Context::default());
    let store = credentials::Store::empty_for_test();
    let mut app = App::with_settings_and_store(&context, Settings::default(), store);
    app.window_positions = WindowPositions::default();
    app.settings_section = SettingsSection::Appearance;
    app.settings_open = true;
    app.log_open = true;
    let now = Instant::now();
    app.transcript.begin_session("Aoede".into(), now);
    app.transcript.append(
        Speaker::Partner,
        "Hello, my name is Ana.".into(),
        None,
        true,
    );
    app.transcript.finish_session(now);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 1000.0),
        )),
        ..Default::default()
    };
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output.textures_delta.clear();
        output = context.egui_ctx.run_ui(raw.clone(), |ui| {
            app.settings_panel(ui.ctx());
            app.log_panel(ui.ctx());
        });
    }
    app.positions_dirty = false;
    let mut text = Vec::new();
    for shape in &output.shapes {
        collect_text(&shape.shape, &mut text);
    }
    output.textures_delta.clear();
    assert!(
        text.iter()
            .any(|value| value == "Open conversation log when a conversation ends"),
        "Rendered text: {text:?}"
    );
    assert!(text.iter().any(|value| value == "Ana"));
    assert!(text.iter().any(|value| value == "Hello, my name is Ana."));
}
#[test]
fn loaded_sessions_keep_duration_voice_and_do_not_appear_as_new_bubbles() {
    let mut record = ChatRecord::new("Spanish".into());
    record.sessions.push(SavedSession {
        voice: "Aoede".into(),
        duration_seconds: 90_000,
        messages: vec![MemoryMessage {
            speaker: Speaker::User,
            text: "Hola".into(),
        }],
    });
    let now = Instant::now();
    let mut transcript = Transcript::from_record(&record, now);
    assert_eq!(transcript.saved_sessions(now)[0].duration_seconds, 90_000);
    assert!(transcript.lines[0].historical);
    transcript.begin_session("Puck".into(), now);
    transcript.append(Speaker::Partner, "Hola".into(), None, false);
    assert_eq!(
        transcript.speaker_name(&transcript.lines[1], UiLanguage::English),
        "AI Partner"
    );
    assert!(!transcript.lines[1].historical);
}
#[test]
fn log_keeps_each_conversations_voice_and_duration_and_clear_keeps_active_clock() {
    let mut transcript = Transcript::default();
    let start = Instant::now();
    transcript.begin_session("Aoede".into(), start);
    transcript.append(Speaker::Partner, "First".into(), Some("same".into()), false);
    transcript.finish_session(start + Duration::from_secs(10));
    transcript.begin_session("Puck".into(), start + Duration::from_secs(12));
    transcript.append(
        Speaker::Partner,
        "Second".into(),
        Some("same".into()),
        false,
    );
    transcript.finish_session(start + Duration::from_secs(75));
    let text = transcript.text(UiLanguage::English);
    assert!(text.contains("Conversation · AI Partner · 00:10\n\nAI Partner: First"));
    assert!(text.contains("Conversation · AI Partner · 01:03\n\nAI Partner: Second"));
    transcript.begin_session("Leda".into(), start + Duration::from_secs(80));
    transcript.clear();
    assert_eq!(transcript.sessions.len(), 1);
    assert_eq!(transcript.sessions[0].voice, "Leda");
    assert_eq!(
        transcript.sessions[0]
            .clock
            .elapsed(start + Duration::from_secs(100))
            .as_secs(),
        20
    );
    assert_eq!(format_duration(Duration::from_secs(3_661)), "1:01:01");
}
#[test]
fn theme_text_remains_legible_on_its_background() {
    fn luminance(color: Color32) -> f32 {
        let channel = |value: u8| {
            let value = value as f32 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }
    for theme in [Theme::Light, Theme::Dark] {
        let colors = palette(theme);
        let background = luminance(colors.paper);
        for foreground in [colors.ink, colors.muted, colors.error] {
            let value = luminance(foreground);
            let contrast = (value.max(background) + 0.05) / (value.min(background) + 0.05);
            assert!(contrast >= 4.5, "{theme:?}: {contrast}");
        }
    }
}
