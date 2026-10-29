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
use eframe::egui::{self, Color32};
use std::{
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

use library_search::LibrarySearch;
use theme::{apply_theme, palette, BLUE};
use transcript::{format_duration, ConversationClock, SpeechIndicator, Transcript};
use windowing::WindowPlacement;

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
    settings_placement: WindowPlacement,
    log_placement: WindowPlacement,
    positions_dirty: bool,
    store: ChatStore,
    selected_chat: Option<ChatRecord>,
    library_open: bool,
    library_query: String,
    library: Vec<ChatRecord>,
    library_search: LibrarySearch,
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
            settings_placement: WindowPlacement::default(),
            log_placement: WindowPlacement::default(),
            positions_dirty: false,
            store: ChatStore::beside_executable(),
            selected_chat: None,
            library_open: false,
            library_query: String::new(),
            library: Vec::new(),
            library_search: LibrarySearch::default(),
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
        if was_active && self.settings.open_log_after_conversation {
            self.library_open = false;
            self.log_open = true;
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
                self.library_search.invalidate();
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
        self.settings_placement.reset();
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

mod events;
mod interface;
mod interface_helpers;
mod library_search;
mod log;
mod partner_name;
mod settings;
mod settings_window;

mod theme;
mod transcript;
mod voices;
mod windowing;

#[cfg(test)]
mod tests;
