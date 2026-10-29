use super::*;

#[derive(Clone, Copy)]
pub(super) struct ConversationClock {
    pub(super) started: Instant,
    pub(super) ended: Option<Instant>,
    pub(super) previous: Duration,
}
impl ConversationClock {
    pub(super) fn elapsed(self, now: Instant) -> Duration {
        self.ended
            .unwrap_or(now)
            .saturating_duration_since(self.started)
            .saturating_add(self.previous)
    }
    pub(super) fn finish(&mut self, now: Instant) {
        if self.ended.is_none() {
            self.ended = Some(now);
        }
    }
}
pub(super) fn format_duration(duration: Duration) -> String {
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
pub(super) struct LogSession {
    pub(super) voice: String,
    pub(super) clock: ConversationClock,
    partner_name: Option<String>,
}

pub(super) struct Line {
    pub(super) speaker: Speaker,
    pub(super) session: usize,
    pub(super) text: String,
    pub(super) id: Option<String>,
    pub(super) updated: Instant,
    pub(super) historical: bool,
}
#[derive(Default)]
pub(super) struct Transcript {
    pub(super) lines: Vec<Line>,
    pub(super) sessions: Vec<LogSession>,
    pub(super) current_session: Option<usize>,
    pub(super) active_user: Option<usize>,
    pub(super) active_partner: Option<usize>,
}

#[derive(Default)]
pub(super) struct SpeechIndicator {
    pub(super) speaking: bool,
    pub(super) started: Option<Instant>,
    pub(super) last_sound: Option<Instant>,
}
impl SpeechIndicator {
    pub(super) fn update(&mut self, now: Instant, level: f32) {
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
    pub(super) fn begin_session(&mut self, voice: String, now: Instant) {
        self.end_turn();
        let partner_name = self
            .sessions
            .last()
            .and_then(|session| session.partner_name.clone());
        self.sessions.push(LogSession {
            voice,
            partner_name,
            clock: ConversationClock {
                started: now,
                ended: None,
                previous: Duration::ZERO,
            },
        });
        self.current_session = Some(self.sessions.len() - 1);
    }
    pub(super) fn finish_session(&mut self, now: Instant) {
        if let Some(index) = self.current_session.take() {
            self.sessions[index].clock.finish(now);
        }
        self.end_turn();
    }
    pub(super) fn speaker_name<'a>(&'a self, line: &Line, language: UiLanguage) -> &'a str {
        if line.speaker == Speaker::User {
            language.text("You")
        } else {
            self.sessions[line.session]
                .partner_name
                .as_deref()
                .unwrap_or(language.text("AI Partner"))
        }
    }
    pub(super) fn partner_name(&self, language: UiLanguage) -> &str {
        self.current_session
            .and_then(|index| self.sessions.get(index))
            .or_else(|| self.sessions.last())
            .and_then(|session| session.partner_name.as_deref())
            .unwrap_or(language.text("AI Partner"))
    }
    pub(super) fn append(
        &mut self,
        speaker: Speaker,
        text: String,
        id: Option<String>,
        replace: bool,
    ) {
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
            self.update_partner_name(index, replace);
        }
    }
    fn update_partner_name(&mut self, line_index: usize, completed: bool) {
        let line = &self.lines[line_index];
        if let Some(name) = super::partner_name::introduced_name(&line.text, completed) {
            self.sessions[line.session].partner_name = Some(name);
        }
    }
    pub(super) fn end_turn(&mut self) {
        if let Some(index) = self.active_partner {
            self.update_partner_name(index, true);
        }
        self.active_user = None;
        self.active_partner = None;
    }
    pub(super) fn clear(&mut self) {
        let active = self
            .current_session
            .map(|index| self.sessions[index].clone());
        *self = Self::default();
        if let Some(session) = active {
            self.sessions.push(session);
            self.current_session = Some(0);
        }
    }
    pub(super) fn text(&self, language: UiLanguage) -> String {
        let now = Instant::now();
        self.sessions
            .iter()
            .enumerate()
            .map(|(index, session)| {
                let mut text = format!(
                    "{} · {} · {}",
                    language.text("Conversation"),
                    session
                        .partner_name
                        .as_deref()
                        .unwrap_or(language.text("AI Partner")),
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
    pub(super) fn from_record(record: &ChatRecord, now: Instant) -> Self {
        let mut transcript = Self::default();
        for session in &record.sessions {
            let index = transcript.sessions.len();
            let partner_name = session
                .messages
                .iter()
                .filter(|message| message.speaker == Speaker::Partner)
                .filter_map(|message| super::partner_name::introduced_name(&message.text, true))
                .last()
                .or_else(|| {
                    transcript
                        .sessions
                        .last()
                        .and_then(|previous| previous.partner_name.clone())
                });
            transcript.sessions.push(LogSession {
                voice: session.voice.clone(),
                partner_name,
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
    pub(super) fn saved_sessions(&self, now: Instant) -> Vec<SavedSession> {
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
