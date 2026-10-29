use super::*;

impl App {
    pub(super) fn update_events(&mut self) {
        let events: Vec<Event> = self
            .receiver
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            self.handle_event(event);
        }
        if self.history_dirty && self.history_last_saved.elapsed() >= Duration::from_secs(1) {
            self.flush_history();
        } else if self.connected && self.history_last_saved.elapsed() >= Duration::from_secs(10) {
            self.history_dirty = true;
            self.flush_history();
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Ready => self.session_ready(),
            Event::Status(message) => {
                self.connected = false;
                self.status = message;
            }
            Event::Error(message) => self.session_error(message),
            Event::Stopped => self.session_stopped(),
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

    fn session_ready(&mut self) {
        if self.stopping {
            return;
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
                self.selected_chat = Some(ChatRecord::new(self.settings.language.clone()));
            }
            self.history_dirty = true;
        }
        self.connected = true;
        self.stopping = false;
        self.error = false;
        self.status = "Listening when you are ready".into();
    }

    fn session_error(&mut self, message: String) {
        self.finish_conversation(Instant::now());
        self.error = true;
        self.connected = false;
        self.status = message;
        self.session = None;
        self.receiver = None;
        self.stopping = false;
    }

    fn session_stopped(&mut self) {
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
}
