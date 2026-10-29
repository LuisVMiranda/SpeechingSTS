use crate::{
    audio::{self, AudioControls, Cue},
    config::{
        InputMode, Provider, Settings, Shortcut, Theme, WindowPositions, GEMINI_VOICES,
        OPENAI_VOICES,
    },
    credentials,
    engine::Session,
    history::{self, ChatRecord, ChatStore, MemoryMessage, SavedSession},
    i18n::UiLanguage,
    protocol::{Event, Speaker},
};
use eframe::egui::{self, Color32, Vec2};
use std::{
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

const INK: Color32 = Color32::from_rgb(25, 40, 58);
const BLUE: Color32 = Color32::from_rgb(35, 99, 225);
const PAPER: Color32 = Color32::from_rgb(250, 252, 255);
const MUTED: Color32 = Color32::from_rgb(86, 105, 125);

#[derive(Clone, Copy)]
struct Palette {
    ink: Color32,
    paper: Color32,
    muted: Color32,
    error: Color32,
}
fn palette(theme: Theme) -> Palette {
    if theme == Theme::Dark {
        Palette {
            ink: Color32::from_rgb(231, 238, 247),
            paper: Color32::from_rgb(20, 27, 38),
            muted: Color32::from_rgb(162, 180, 201),
            error: Color32::from_rgb(255, 165, 150),
        }
    } else {
        Palette {
            ink: INK,
            paper: PAPER,
            muted: MUTED,
            error: Color32::from_rgb(170, 63, 52),
        }
    }
}

fn apply_theme(ctx: &egui::Context, theme: Theme) {
    let colors = palette(theme);
    let mut style = (*ctx.global_style()).clone();
    style.visuals = if theme == Theme::Dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    style.visuals.override_text_color = Some(colors.ink);
    style.visuals.panel_fill = colors.paper;
    style.visuals.window_fill = colors.paper;
    style.visuals.widgets.noninteractive.bg_fill = colors.paper;
    style.visuals.widgets.inactive.bg_fill = if theme == Theme::Dark {
        Color32::from_rgb(38, 50, 67)
    } else {
        Color32::from_rgb(236, 242, 250)
    };
    style.visuals.widgets.hovered.bg_fill = if theme == Theme::Dark {
        Color32::from_rgb(54, 74, 101)
    } else {
        Color32::from_rgb(220, 233, 250)
    };
    style.visuals.selection.bg_fill = if theme == Theme::Dark {
        Color32::from_rgb(45, 79, 128)
    } else {
        Color32::from_rgb(169, 201, 246)
    };
    style.spacing.item_spacing = Vec2::new(8.0, 9.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.button_padding = Vec2::new(11.0, 6.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
    ctx.set_global_style(style);
}

#[derive(Clone, Copy)]
struct ConversationClock {
    started: Instant,
    ended: Option<Instant>,
    previous: Duration,
}
impl ConversationClock {
    fn elapsed(self, now: Instant) -> Duration {
        self.ended
            .unwrap_or(now)
            .saturating_duration_since(self.started)
            .saturating_add(self.previous)
    }
    fn finish(&mut self, now: Instant) {
        if self.ended.is_none() {
            self.ended = Some(now);
        }
    }
}
fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            (seconds / 60) % 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
}

#[derive(Clone)]
struct LogSession {
    voice: String,
    clock: ConversationClock,
}

struct Line {
    speaker: Speaker,
    session: usize,
    text: String,
    id: Option<String>,
    updated: Instant,
    historical: bool,
}
#[derive(Default)]
struct Transcript {
    lines: Vec<Line>,
    sessions: Vec<LogSession>,
    current_session: Option<usize>,
    active_user: Option<usize>,
    active_partner: Option<usize>,
}

#[derive(Default)]
struct SpeechIndicator {
    speaking: bool,
    started: Option<Instant>,
    last_sound: Option<Instant>,
}
impl SpeechIndicator {
    fn update(&mut self, now: Instant, level: f32) {
        let recent = self
            .last_sound
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(150));
        if level > 0.015 {
            if !recent {
                self.started = Some(now);
            }
            self.last_sound = Some(now);
            if self
                .started
                .is_some_and(|start| now.duration_since(start) >= Duration::from_millis(100))
            {
                self.speaking = true;
            }
        } else if self.speaking
            && self
                .last_sound
                .is_some_and(|last| now.duration_since(last) >= Duration::from_millis(750))
        {
            self.speaking = false;
            self.started = None;
        }
    }
}
impl Transcript {
    fn begin_session(&mut self, voice: String, now: Instant) {
        self.end_turn();
        self.sessions.push(LogSession {
            voice,
            clock: ConversationClock {
                started: now,
                ended: None,
                previous: Duration::ZERO,
            },
        });
        self.current_session = Some(self.sessions.len() - 1);
    }
    fn finish_session(&mut self, now: Instant) {
        if let Some(index) = self.current_session.take() {
            self.sessions[index].clock.finish(now);
        }
        self.end_turn();
    }
    fn speaker_name<'a>(&'a self, line: &Line, language: UiLanguage) -> &'a str {
        if line.speaker == Speaker::User {
            language.text("You")
        } else {
            &self.sessions[line.session].voice
        }
    }
    fn append(&mut self, speaker: Speaker, text: String, id: Option<String>, replace: bool) {
        let Some(session) = self.current_session else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let matching_id = id.as_ref().and_then(|id| {
            self.lines.iter().position(|l| {
                l.session == session && l.speaker == speaker && l.id.as_ref() == Some(id)
            })
        });
        let active = if speaker == Speaker::User {
            self.active_user
        } else {
            self.active_partner
        };
        let index = matching_id
            .or_else(|| if id.is_none() { active } else { None })
            .unwrap_or_else(|| {
                self.lines.push(Line {
                    speaker,
                    session,
                    text: String::new(),
                    id: id.clone(),
                    updated: Instant::now(),
                    historical: false,
                });
                self.lines.len() - 1
            });
        let line = &mut self.lines[index];
        if replace || (!line.text.is_empty() && text.starts_with(&line.text)) {
            line.text = text;
        } else {
            line.text.push_str(&text);
        }
        line.updated = Instant::now();
        if speaker == Speaker::User {
            self.active_user = Some(index);
        } else {
            self.active_partner = Some(index);
            self.active_user = None;
        }
    }
    fn end_turn(&mut self) {
        self.active_user = None;
        self.active_partner = None;
    }
    fn clear(&mut self) {
        let active = self
            .current_session
            .map(|index| self.sessions[index].clone());
        *self = Self::default();
        if let Some(session) = active {
            self.sessions.push(session);
            self.current_session = Some(0);
        }
    }
    fn text(&self, language: UiLanguage) -> String {
        let now = Instant::now();
        self.sessions
            .iter()
            .enumerate()
            .map(|(index, session)| {
                let mut text = format!(
                    "{} · {} · {}",
                    language.text("Conversation"),
                    session.voice,
                    format_duration(session.clock.elapsed(now))
                );
                for line in self.lines.iter().filter(|line| line.session == index) {
                    text.push_str(&format!(
                        "\n\n{}: {}",
                        self.speaker_name(line, language),
                        line.text.trim()
                    ));
                }
                text
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }
    fn from_record(record: &ChatRecord, now: Instant) -> Self {
        let mut transcript = Self::default();
        for session in &record.sessions {
            let index = transcript.sessions.len();
            transcript.sessions.push(LogSession {
                voice: session.voice.clone(),
                clock: ConversationClock {
                    started: now,
                    ended: Some(now),
                    previous: Duration::from_secs(session.duration_seconds),
                },
            });
            for message in &session.messages {
                transcript.lines.push(Line {
                    speaker: message.speaker,
                    session: index,
                    text: message.text.clone(),
                    id: None,
                    updated: now,
                    historical: true,
                });
            }
        }
        transcript
    }
    fn saved_sessions(&self, now: Instant) -> Vec<SavedSession> {
        self.sessions
            .iter()
            .enumerate()
            .map(|(index, session)| SavedSession {
                voice: session.voice.clone(),
                duration_seconds: session.clock.elapsed(now).as_secs(),
                messages: self
                    .lines
                    .iter()
                    .filter(|line| line.session == index)
                    .map(|line| MemoryMessage {
                        speaker: line.speaker,
                        text: line.text.clone(),
                    })
                    .collect(),
            })
            .collect()
    }
}

pub struct App {
    settings: Settings,
    credential_store: credentials::Store,
    gemini_key: String,
    openai_key: String,
    gemini_key_saved: bool,
    openai_key_saved: bool,
    editing_key: Option<Provider>,
    key_feedback: String,
    session: Option<Session>,
    receiver: Option<Receiver<Event>>,
    transcript: Transcript,
    status: String,
    error: bool,
    connected: bool,
    stopping: bool,
    muted: bool,
    settings_open: bool,
    settings_section: SettingsSection,
    log_open: bool,
    log_feedback: String,
    initial_positioned: bool,
    indicator: SpeechIndicator,
    cue_started: bool,
    language_suggestions_open: bool,
    language_input: String,
    active_input_mode: InputMode,
    talking: bool,
    idle_since: Instant,
    clock: Option<ConversationClock>,
    active_voice: String,
    applied_theme: Theme,
    window_positions: WindowPositions,
    positions_dirty: bool,
    store: ChatStore,
    selected_chat: Option<ChatRecord>,
    library_open: bool,
    library_query: String,
    library: Vec<ChatRecord>,
    library_warning: String,
    history_dirty: bool,
    history_last_saved: Instant,
    history_truncated: bool,
    active_shortcut: Shortcut,
    shortcut_capture: bool,
    rename_active: bool,
    rename_draft: String,
    delete_confirm: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SettingsSection {
    #[default]
    Practice,
    Microphone,
    Provider,
    Appearance,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();
        Self::with_settings_and_store(cc, settings, credentials::Store::default())
    }
    fn with_settings_and_store(
        cc: &eframe::CreationContext<'_>,
        settings: Settings,
        credential_store: credentials::Store,
    ) -> Self {
        let language_input = settings.ui_language.text(&settings.language).to_owned();
        let applied_theme = settings.theme;
        apply_theme(&cc.egui_ctx, applied_theme);
        let gemini = credential_store.load(Provider::Gemini);
        let openai = credential_store.load(Provider::OpenAi);
        let key_feedback = gemini
            .as_ref()
            .err()
            .or_else(|| openai.as_ref().err())
            .cloned()
            .unwrap_or_default();
        let gemini_key_saved =
            gemini.is_ok_and(|key| key.is_some_and(|key| !key.trim().is_empty()));
        let openai_key_saved =
            openai.is_ok_and(|key| key.is_some_and(|key| !key.trim().is_empty()));
        Self {
            settings,
            credential_store,
            gemini_key: String::new(),
            openai_key: String::new(),
            gemini_key_saved,
            openai_key_saved,
            editing_key: None,
            key_feedback,
            session: None,
            receiver: None,
            transcript: Transcript::default(),
            status: "Ready when you are".into(),
            error: false,
            connected: false,
            stopping: false,
            muted: false,
            settings_open: false,
            settings_section: SettingsSection::Practice,
            log_open: false,
            log_feedback: String::new(),
            initial_positioned: false,
            indicator: SpeechIndicator::default(),
            cue_started: false,
            language_suggestions_open: false,
            language_input,
            active_input_mode: InputMode::VoiceActivated,
            talking: false,
            idle_since: Instant::now(),
            clock: None,
            active_voice: String::new(),
            applied_theme,
            window_positions: WindowPositions::load(),
            positions_dirty: false,
            store: ChatStore::beside_executable(),
            selected_chat: None,
            library_open: false,
            library_query: String::new(),
            library: Vec::new(),
            library_warning: String::new(),
            history_dirty: false,
            history_last_saved: Instant::now(),
            history_truncated: false,
            active_shortcut: Shortcut::default(),
            shortcut_capture: false,
            rename_active: false,
            rename_draft: String::new(),
            delete_confirm: false,
        }
    }
    fn start(&mut self) {
        let Some(language) =
            crate::languages::canonical(self.settings.provider, &self.settings.language)
        else {
            self.status = "Choose a supported language in settings.".into();
            self.error = true;
            self.settings_open = true;
            self.settings_section = SettingsSection::Practice;
            return;
        };
        self.settings.language = language.to_owned();
        self.active_input_mode = self.settings.input_mode;
        self.active_shortcut = self.settings.push_to_talk_shortcut.clone();
        if !self.persist_pending_keys() {
            return;
        }
        let key = match self.credential_store.load(self.settings.provider) {
            Ok(Some(key)) => key,
            Ok(None) => String::new(),
            Err(error) => {
                self.key_feedback = error.clone();
                self.status = error;
                self.error = true;
                self.settings_open = true;
                self.settings_section = SettingsSection::Provider;
                return;
            }
        };
        if key.trim().is_empty() {
            self.status = "Add your API key in settings.".into();
            self.settings_open = true;
            self.settings_section = SettingsSection::Provider;
            return;
        }
        if self.settings.model().trim().is_empty() || self.settings.voice().trim().is_empty() {
            self.status = "Choose a language, model, and voice in settings.".into();
            self.error = true;
            self.settings_open = true;
            return;
        }
        if let Err(error) = self.store.ensure() {
            self.status = error;
            self.error = true;
            return;
        }
        if !self.flush_history() {
            return;
        }
        let memory = self
            .selected_chat
            .as_ref()
            .map(|chat| chat.context().0)
            .unwrap_or_default();
        let (tx, rx) = mpsc::channel();
        self.active_voice = self.settings.voice().to_owned();
        self.clock = None;
        self.session = Some(Session::start(self.settings.clone(), key, memory, tx));
        self.receiver = Some(rx);
        self.status = "Connecting…".into();
        self.error = false;
        self.connected = false;
        self.stopping = false;
        self.muted = false;
        self.indicator = SpeechIndicator::default();
        self.cue_started = false;
        self.talking = false;
    }
    fn stop(&mut self) {
        self.freeze_conversation(Instant::now());
        self.history_dirty = self.selected_chat.is_some();
        self.flush_history();
        if self.cue_started {
            audio::play_cue(Cue::End);
            self.cue_started = false;
        }
        if let Some(session) = &self.session {
            session.stop();
        }
        self.status = "Ending conversation…".into();
        self.stopping = true;
        self.connected = false;
    }
    fn update_events(&mut self) {
        let events: Vec<Event> = self
            .receiver
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Ready => {
                    if self.stopping {
                        continue;
                    }
                    if !self.cue_started {
                        audio::play_cue(Cue::Start);
                        self.cue_started = true;
                    }
                    if self.clock.is_none() {
                        let now = Instant::now();
                        self.clock = Some(ConversationClock {
                            started: now,
                            ended: None,
                            previous: Duration::ZERO,
                        });
                        self.transcript
                            .begin_session(self.active_voice.clone(), now);
                        if self.selected_chat.is_none() {
                            self.selected_chat =
                                Some(ChatRecord::new(self.settings.language.clone()));
                        }
                        self.history_dirty = true;
                    }
                    self.connected = true;
                    self.stopping = false;
                    self.error = false;
                    self.status = "Listening when you are ready".into();
                }
                Event::Status(message) => {
                    self.connected = false;
                    self.status = message;
                }
                Event::Error(message) => {
                    self.finish_conversation(Instant::now());
                    self.error = true;
                    self.connected = false;
                    self.status = message;
                    self.session = None;
                    self.receiver = None;
                    self.stopping = false;
                }
                Event::Stopped => {
                    self.finish_conversation(Instant::now());
                    self.session = None;
                    self.receiver = None;
                    self.connected = false;
                    self.stopping = false;
                    self.muted = false;
                    self.cue_started = false;
                    self.indicator = SpeechIndicator::default();
                    self.talking = false;
                    self.idle_since = Instant::now();
                    if !self.error {
                        self.status = "Conversation ended".into();
                    }
                    self.transcript.end_turn();
                }
                Event::Transcript {
                    speaker,
                    text,
                    id,
                    replace,
                } => {
                    self.transcript.append(speaker, text, id, replace);
                    self.history_dirty = self.selected_chat.is_some();
                }
                Event::TurnEnd => self.transcript.end_turn(),
            }
        }
        if self.history_dirty && self.history_last_saved.elapsed() >= Duration::from_secs(1) {
            self.flush_history();
        } else if self.connected && self.history_last_saved.elapsed() >= Duration::from_secs(10) {
            self.history_dirty = true;
            self.flush_history();
        }
    }
    fn levels(&self) -> (f32, f32) {
        self.session
            .as_ref()
            .map(|s| {
                (
                    AudioControls::level(&s.audio.input_level),
                    AudioControls::level(&s.audio.output_level),
                )
            })
            .unwrap_or((0.0, 0.0))
    }
    fn finish_conversation(&mut self, now: Instant) {
        let was_active = self.transcript.current_session.is_some();
        self.freeze_conversation(now);
        self.transcript.finish_session(now);
        self.history_dirty |= was_active && self.selected_chat.is_some();
        if self.history_dirty {
            self.flush_history();
        }
    }
    fn freeze_conversation(&mut self, now: Instant) {
        if let Some(clock) = &mut self.clock {
            clock.finish(now);
        }
        if let Some(index) = self.transcript.current_session {
            self.transcript.sessions[index].clock.finish(now);
        }
    }
    fn save_positions(&mut self) {
        if self.positions_dirty {
            match self.window_positions.save() {
                Ok(()) => self.positions_dirty = false,
                Err(error) => {
                    self.status = format!("Could not save window positions: {error}");
                    self.error = true;
                }
            }
        }
    }
    fn flush_history(&mut self) -> bool {
        if !self.history_dirty {
            return true;
        }
        let Some(chat) = &mut self.selected_chat else {
            self.history_dirty = false;
            return true;
        };
        chat.sessions = self.transcript.saved_sessions(Instant::now());
        self.history_truncated = chat.context().1;
        chat.updated_at = history::timestamp();
        chat.refresh_title();
        self.history_last_saved = Instant::now();
        match self.store.save(chat) {
            Ok(()) => {
                self.history_dirty = false;
                true
            }
            Err(error) => {
                self.log_feedback = error.clone();
                self.status = error;
                self.error = true;
                false
            }
        }
    }
    fn refresh_library(&mut self) {
        match self.store.list() {
            Ok((records, skipped)) => {
                self.library = records;
                self.library_warning = if skipped == 0 {
                    String::new()
                } else {
                    format!("{skipped} conversation file(s) could not be read.")
                };
            }
            Err(error) => self.library_warning = error,
        }
    }
    fn save_pending_keys(&mut self) -> Result<(), String> {
        for provider in [Provider::Gemini, Provider::OpenAi] {
            let draft = match provider {
                Provider::Gemini => &mut self.gemini_key,
                Provider::OpenAi => &mut self.openai_key,
            };
            if !draft.trim().is_empty() {
                self.credential_store.save(provider, draft)?;
                draft.clear();
                match provider {
                    Provider::Gemini => self.gemini_key_saved = true,
                    Provider::OpenAi => self.openai_key_saved = true,
                }
                if self.editing_key == Some(provider) {
                    self.editing_key = None;
                }
            }
        }
        self.key_feedback.clear();
        Ok(())
    }
    fn persist_pending_keys(&mut self) -> bool {
        match self.save_pending_keys() {
            Ok(()) => true,
            Err(error) => {
                self.key_feedback = error.clone();
                self.status = error;
                self.error = true;
                self.settings_open = true;
                self.settings_section = SettingsSection::Provider;
                false
            }
        }
    }
    fn forget_key(&mut self, provider: Provider) {
        match self.credential_store.forget(provider) {
            Ok(()) => {
                match provider {
                    Provider::Gemini => {
                        self.gemini_key_saved = false;
                        self.gemini_key.clear();
                    }
                    Provider::OpenAi => {
                        self.openai_key_saved = false;
                        self.openai_key.clear();
                    }
                }
                if self.editing_key == Some(provider) {
                    self.editing_key = None;
                }
                self.key_feedback = "Saved API key removed.".into();
            }
            Err(error) => self.key_feedback = error,
        }
    }
    fn close_settings(&mut self) {
        if !self.persist_pending_keys() {
            return;
        }
        self.settings_open = false;
        self.shortcut_capture = false;
        self.save_positions();
    }
    fn rename_chat(&mut self) {
        if self.session.is_some() || !self.flush_history() {
            return;
        }
        let Some(mut chat) = self.selected_chat.clone() else {
            return;
        };
        match chat
            .rename(&self.rename_draft)
            .and_then(|_| self.store.save(&chat))
        {
            Ok(()) => {
                self.selected_chat = Some(chat);
                self.rename_active = false;
                self.log_feedback = "Conversation renamed.".into();
            }
            Err(error) => self.log_feedback = error,
        }
    }
    fn delete_chat(&mut self) {
        if self.session.is_some() {
            return;
        }
        let Some(chat) = &self.selected_chat else {
            return;
        };
        match self.store.delete(&chat.id) {
            Ok(()) => {
                self.selected_chat = None;
                self.transcript = Transcript::default();
                self.history_dirty = false;
                self.history_truncated = false;
                self.clock = None;
                self.rename_active = false;
                self.log_feedback = "Conversation deleted.".into();
                self.status = "Ready for a new conversation".into();
                self.error = false;
                self.refresh_library();
            }
            Err(error) => self.log_feedback = error,
        }
        self.delete_confirm = false;
    }
    fn new_chat(&mut self) {
        if self.session.is_some() || !self.flush_history() {
            return;
        }
        self.transcript = Transcript::default();
        self.selected_chat = None;
        self.history_truncated = false;
        self.clock = None;
        self.library_open = false;
        self.rename_active = false;
        self.log_feedback.clear();
        self.status = "Ready for a new conversation".into();
        self.error = false;
    }
    fn select_chat(&mut self, id: &str) {
        if self.session.is_some() || !self.flush_history() {
            return;
        }
        match self.store.load(id) {
            Ok(chat) => {
                self.history_truncated = chat.context().1;
                self.transcript = Transcript::from_record(&chat, Instant::now());
                self.selected_chat = Some(chat);
                self.clock = None;
                self.library_open = false;
                self.rename_active = false;
                self.log_feedback.clear();
                self.status = "Ready to continue".into();
                self.error = false;
            }
            Err(error) => self.log_feedback = error,
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.finish_conversation(Instant::now());
        if self.positions_dirty {
            let _ = self.window_positions.save();
        }
    }
}

mod interface;
mod log;
mod settings;

#[cfg(test)]
mod tests {
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
        let drafts_empty = (
            restarted.gemini_key.is_empty(),
            restarted.openai_key.is_empty(),
        );
        restarted.start();
        let start_status = restarted.status.clone();
        store.forget(Provider::Gemini).unwrap();
        store.forget(Provider::OpenAi).unwrap();

        assert_eq!(saved_flags, (true, true));
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
            "Conversation · Aoede · 01:11\n\nYou: Hello!\n\nAoede: Hi."
        );
        assert_eq!(
            t.text(UiLanguage::Spanish),
            "Conversación · Aoede · 01:11\n\nTú: Hello!\n\nAoede: Hi."
        );
        assert_eq!(
            t.text(UiLanguage::PortugueseBrazil),
            "Conversa · Aoede · 01:11\n\nVocê: Hello!\n\nAoede: Hi."
        );
        t.clear();
        assert!(t.lines.is_empty());
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
            "Puck"
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
        assert!(text.contains("Conversation · Aoede · 00:10\n\nAoede: First"));
        assert!(text.contains("Conversation · Puck · 01:03\n\nPuck: Second"));
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
}
