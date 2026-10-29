use crate::protocol::Speaker;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_CONTEXT_CHARS: usize = 48_000;
const MAX_CONTEXT_MESSAGES: usize = 120;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryMessage {
    pub speaker: Speaker,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedSession {
    pub voice: String,
    pub duration_seconds: u64,
    pub messages: Vec<MemoryMessage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatRecord {
    pub schema_version: u8,
    pub id: String,
    pub title: String,
    pub language: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub sessions: Vec<SavedSession>,
}

impl ChatRecord {
    pub fn new(language: String) -> Self {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            schema_version: 1,
            id: format!("{}-{}", elapsed.as_nanos(), std::process::id()),
            title: "New conversation".into(),
            language,
            created_at: elapsed.as_secs(),
            updated_at: elapsed.as_secs(),
            sessions: Vec::new(),
        }
    }

    pub fn refresh_title(&mut self) {
        if let Some(message) = self
            .sessions
            .iter()
            .flat_map(|s| &s.messages)
            .find(|m| m.speaker == Speaker::User && !m.text.trim().is_empty())
        {
            let text = message
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let mut chars = text.chars();
            let title: String = chars.by_ref().take(60).collect();
            self.title = if chars.next().is_some() {
                format!("{title}…")
            } else {
                title
            };
        } else {
            self.title = "New conversation".into();
        }
    }

    // Keep the newest messages, and preserve role order. A resumed Gemini history
    // begins with a learner turn even when older history has been truncated.
    pub fn context(&self) -> (Vec<MemoryMessage>, bool) {
        let all: Vec<_> = self
            .sessions
            .iter()
            .flat_map(|s| s.messages.iter())
            .filter(|m| !m.text.trim().is_empty())
            .collect();
        let mut chosen = Vec::new();
        let mut remaining = MAX_CONTEXT_CHARS;
        let mut truncated = false;
        for message in all.iter().rev() {
            if chosen.len() >= MAX_CONTEXT_MESSAGES || remaining == 0 {
                truncated = true;
                break;
            }
            let length = message.text.chars().count();
            let text = if length > remaining {
                truncated = true;
                message
                    .text
                    .chars()
                    .rev()
                    .take(remaining)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            } else {
                message.text.clone()
            };
            remaining = remaining.saturating_sub(length);
            chosen.push(MemoryMessage {
                speaker: message.speaker,
                text,
            });
        }
        chosen.reverse();
        while chosen.first().is_some_and(|m| m.speaker != Speaker::User) {
            chosen.remove(0);
            truncated = true;
        }
        (chosen, truncated)
    }
}

pub struct ChatStore {
    root: PathBuf,
}
impl ChatStore {
    pub fn beside_executable() -> Self {
        let root = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("conversations");
        Self { root }
    }
    pub fn ensure(&self) -> Result<(), String> {
        fs::create_dir_all(&self.root)
            .map_err(|e| format!("Cannot create the conversations folder: {e}"))
    }
    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return Err("Invalid conversation ID.".into());
        }
        Ok(self.root.join(format!("chat-{id}.json")))
    }
    pub fn save(&self, record: &ChatRecord) -> Result<(), String> {
        self.ensure()?;
        let path = self.path(&record.id)?;
        let temp = path.with_extension("json.tmp");
        let data = serde_json::to_vec(record).map_err(|e| e.to_string())?;
        fs::write(&temp, data).map_err(|e| format!("Could not save conversation: {e}"))?;
        fs::rename(&temp, &path).map_err(|e| format!("Could not finish saving conversation: {e}"))
    }
    pub fn load(&self, id: &str) -> Result<ChatRecord, String> {
        let data =
            fs::read(self.path(id)?).map_err(|e| format!("Could not open conversation: {e}"))?;
        let record: ChatRecord = serde_json::from_slice(&data)
            .map_err(|e| format!("Could not read conversation: {e}"))?;
        if record.id != id || record.schema_version != 1 {
            return Err("Unsupported conversation file.".into());
        }
        Ok(record)
    }
    pub fn list(&self) -> Result<(Vec<ChatRecord>, usize), String> {
        self.ensure()?;
        let mut records = Vec::new();
        let mut skipped = 0;
        for entry in fs::read_dir(&self.root).map_err(|e| e.to_string())? {
            let Ok(entry) = entry else {
                skipped += 1;
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(id) = name
                .strip_prefix("chat-")
                .and_then(|n| n.strip_suffix(".json"))
            {
                match self.load(id) {
                    Ok(record) => records.push(record),
                    Err(_) => skipped += 1,
                }
            }
        }
        records.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok((records, skipped))
    }
}

pub fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_load_and_search_metadata_stay_separate() {
        let root = std::env::temp_dir().join(format!(
            "speeching-history-test-{}",
            ChatRecord::new("English".into()).id
        ));
        let store = ChatStore { root: root.clone() };
        let mut chat = ChatRecord::new("English".into());
        chat.sessions.push(SavedSession {
            voice: "Aoede".into(),
            duration_seconds: 8,
            messages: vec![MemoryMessage {
                speaker: Speaker::User,
                text: "Hello there".into(),
            }],
        });
        chat.refresh_title();
        store.save(&chat).unwrap();
        chat.sessions[0].messages.push(MemoryMessage {
            speaker: Speaker::Partner,
            text: "Hi!".into(),
        });
        store.save(&chat).unwrap();
        let loaded = store.load(&chat.id).unwrap();
        assert_eq!(loaded.title, "Hello there");
        assert_eq!(loaded.sessions[0].messages.len(), 2);
        assert_eq!(store.list().unwrap().0.len(), 1);
        assert!(store.load("../secret").is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn context_is_unicode_safe_bounded_and_chronological() {
        let mut chat = ChatRecord::new("English".into());
        let messages = (0..140)
            .map(|i| MemoryMessage {
                speaker: if i % 2 == 0 {
                    Speaker::User
                } else {
                    Speaker::Partner
                },
                text: format!("{i}:{}", "é".repeat(500)),
            })
            .collect();
        chat.sessions.push(SavedSession {
            voice: "Aoede".into(),
            duration_seconds: 0,
            messages,
        });
        let (messages, truncated) = chat.context();
        assert!(truncated);
        assert_eq!(messages.first().unwrap().speaker, Speaker::User);
        assert!(messages.len() <= MAX_CONTEXT_MESSAGES);
        assert!(
            messages
                .iter()
                .map(|m| m.text.chars().count())
                .sum::<usize>()
                <= MAX_CONTEXT_CHARS
        );
        assert!(messages.last().unwrap().text.starts_with("139:"));
    }
}
