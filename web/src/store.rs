use crate::crypto;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Store(Mutex<Connection>);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub speaker: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub language: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub duration_seconds: i64,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        let db = Connection::open(path).map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS users (name TEXT PRIMARY KEY, password_hash TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS secrets (owner TEXT NOT NULL, provider TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(owner, provider));
            CREATE TABLE IF NOT EXISTS chats (id TEXT PRIMARY KEY, owner TEXT NOT NULL, title TEXT NOT NULL, custom INTEGER NOT NULL DEFAULT 0, language TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, duration INTEGER NOT NULL DEFAULT 0);
            CREATE INDEX IF NOT EXISTS chats_owner ON chats(owner, updated_at);
            CREATE TABLE IF NOT EXISTS messages (id INTEGER PRIMARY KEY, chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE, speaker TEXT NOT NULL, text TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS owner_revisions (owner TEXT PRIMARY KEY, revision INTEGER NOT NULL);
            CREATE TRIGGER IF NOT EXISTS chats_revision_insert AFTER INSERT ON chats BEGIN INSERT INTO owner_revisions(owner,revision) VALUES(NEW.owner,1) ON CONFLICT(owner) DO UPDATE SET revision=revision+1; END;
            CREATE TRIGGER IF NOT EXISTS chats_revision_update AFTER UPDATE ON chats BEGIN INSERT INTO owner_revisions(owner,revision) VALUES(NEW.owner,1) ON CONFLICT(owner) DO UPDATE SET revision=revision+1; END;
            CREATE TRIGGER IF NOT EXISTS chats_revision_delete AFTER DELETE ON chats BEGIN INSERT INTO owner_revisions(owner,revision) VALUES(OLD.owner,1) ON CONFLICT(owner) DO UPDATE SET revision=revision+1; END;")
            .map_err(|e| e.to_string())?;
        migrate_messages(&db)?;
        Ok(Self(Mutex::new(db)))
    }

    pub fn put_user(&self, name: &str, hash: &str) -> Result<(), String> {
        let db = self.0.lock().map_err(|_| "Database unavailable")?;
        db.execute("INSERT INTO users(name,password_hash) VALUES(?1,?2) ON CONFLICT(name) DO UPDATE SET password_hash=excluded.password_hash", params![name,hash]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn create_user(&self, name: &str, hash: &str) -> Result<bool, String> {
        let db = self.0.lock().map_err(|_| "Database unavailable")?;
        db.execute(
            "INSERT OR IGNORE INTO users(name,password_hash) VALUES(?1,?2)",
            params![name, hash],
        )
        .map(|changed| changed == 1)
        .map_err(|e| e.to_string())
    }

    pub fn password_hash(&self, name: &str) -> Result<Option<String>, String> {
        let db = self.0.lock().map_err(|_| "Database unavailable")?;
        db.query_row(
            "SELECT password_hash FROM users WHERE name=?1",
            [name],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    pub fn put_secret(&self, owner: &str, provider: &str, value: &str) -> Result<(), String> {
        let db = self.0.lock().map_err(|_| "Database unavailable")?;
        db.execute("INSERT INTO secrets(owner,provider,value) VALUES(?1,?2,?3) ON CONFLICT(owner,provider) DO UPDATE SET value=excluded.value", params![owner,provider,value]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn secret(&self, owner: &str, provider: &str) -> Result<Option<String>, String> {
        let db = self.0.lock().map_err(|_| "Database unavailable")?;
        db.query_row(
            "SELECT value FROM secrets WHERE owner=?1 AND provider=?2",
            params![owner, provider],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    #[cfg(test)]
    pub fn list(&self, owner: &str) -> Result<Vec<Chat>, String> {
        self.list_page(owner, 0, "", "newest").map(|page| page.0)
    }

    pub fn list_page(
        &self,
        owner: &str,
        offset: u32,
        search: &str,
        sort: &str,
    ) -> Result<(Vec<Chat>, bool, i64), String> {
        let db = self.0.lock().map_err(|_| "Database unavailable")?;
        let revision = owner_revision(&db, owner)?;
        let order = match sort {
            "oldest" => "updated_at ASC, id ASC",
            "title" => "title COLLATE NOCASE ASC, id ASC",
            _ => "updated_at DESC, id DESC",
        };
        let sql = format!("SELECT id,title,language,created_at,updated_at,duration FROM chats WHERE owner=?1 AND instr(lower(title || ' ' || language),lower(?2))>0 ORDER BY {order} LIMIT 101 OFFSET ?3");
        let mut stmt = db.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![owner, search, offset], |r| {
                Ok(Chat {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    language: r.get(2)?,
                    created_at: r.get(3)?,
                    updated_at: r.get(4)?,
                    duration_seconds: r.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut chats = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let more = chats.len() > 100;
        chats.truncate(100);
        Ok((chats, more, revision))
    }

    pub fn create(&self, owner: &str, language: &str) -> Result<String, String> {
        let id = crypto::random_token(18);
        let db = self.0.lock().map_err(|_| "Database unavailable")?;
        db.execute("INSERT INTO chats(id,owner,title,language,created_at,updated_at) VALUES(?1,?2,'New conversation',?3,?4,?4)", params![id,owner,language,now()]).map_err(|e| e.to_string())?;
        Ok(id)
    }

    pub fn chat(&self, owner: &str, id: &str) -> Result<Option<Chat>, String> {
        let db = self.0.lock().map_err(|_| "Database unavailable")?;
        db.query_row("SELECT id,title,language,created_at,updated_at,duration FROM chats WHERE owner=?1 AND id=?2", params![owner,id], |r| Ok(Chat { id:r.get(0)?, title:r.get(1)?, language:r.get(2)?, created_at:r.get(3)?, updated_at:r.get(4)?, duration_seconds:r.get(5)? })).optional().map_err(|e| e.to_string())
    }

    pub fn messages(&self, owner: &str, id: &str) -> Result<Option<Vec<Message>>, String> {
        let db = self.0.lock().map_err(|_| "Database unavailable")?;
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM chats WHERE owner=?1 AND id=?2)",
                params![owner, id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !exists {
            return Ok(None);
        }
        let mut stmt = db.prepare("SELECT speaker,text FROM (SELECT id,speaker,text FROM messages WHERE chat_id=?1 ORDER BY id DESC LIMIT 500) ORDER BY id").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([id], |r| {
                Ok(Message {
                    speaker: r.get(0)?,
                    text: r.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map(Some)
            .map_err(|e| e.to_string())
    }

    pub fn message_page(
        &self,
        owner: &str,
        id: &str,
        offset: u32,
    ) -> Result<Option<(Vec<Message>, bool, i64)>, String> {
        let db = self.0.lock().map_err(|_| "Database unavailable")?;
        let revision = owner_revision(&db, owner)?;
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM chats WHERE owner=?1 AND id=?2)",
                params![owner, id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !exists {
            return Ok(None);
        }
        let mut stmt = db.prepare("SELECT speaker,text FROM messages WHERE chat_id=?1 ORDER BY id DESC LIMIT 101 OFFSET ?2").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![id, offset], |r| {
                Ok(Message {
                    speaker: r.get(0)?,
                    text: r.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut messages = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let more = messages.len() > 100;
        messages.truncate(100);
        messages.reverse();
        Ok(Some((messages, more, revision)))
    }

    pub fn rename(&self, owner: &str, id: &str, title: &str) -> Result<bool, String> {
        let title = title.trim();
        if title.is_empty() || title.chars().count() > 100 {
            return Err("Use a name between 1 and 100 characters.".into());
        }
        let db = self.0.lock().map_err(|_| "Database unavailable")?;
        db.execute(
            "UPDATE chats SET title=?1,custom=1,updated_at=?2 WHERE id=?3 AND owner=?4",
            params![title, now(), id, owner],
        )
        .map(|n| n > 0)
        .map_err(|e| e.to_string())
    }

    pub fn delete(&self, owner: &str, id: &str) -> Result<bool, String> {
        let db = self.0.lock().map_err(|_| "Database unavailable")?;
        db.execute(
            "DELETE FROM chats WHERE id=?1 AND owner=?2",
            params![id, owner],
        )
        .map(|n| n > 0)
        .map_err(|e| e.to_string())
    }

    #[cfg(test)]
    pub fn append_session(
        &self,
        owner: &str,
        id: &str,
        messages: &[Message],
        seconds: u64,
    ) -> Result<(), String> {
        self.checkpoint(owner, id, &crypto::random_token(18), messages, seconds)
    }

    pub fn checkpoint(
        &self,
        owner: &str,
        id: &str,
        session: &str,
        messages: &[Message],
        seconds: u64,
    ) -> Result<(), String> {
        let mut db = self.0.lock().map_err(|_| "Database unavailable")?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let previous: i64 = tx
            .query_row(
                "SELECT duration FROM chat_sessions WHERE id=?1 AND chat_id=?2",
                params![session, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or(0);
        let duration = seconds.min(86_400) as i64;
        let changed = tx
            .execute(
                "UPDATE chats SET duration=duration+?1,updated_at=?2 WHERE id=?3 AND owner=?4",
                params![(duration - previous).max(0), now(), id, owner],
            )
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err("Conversation no longer exists.".into());
        }
        tx.execute("INSERT INTO chat_sessions(id,chat_id,duration) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET duration=MAX(duration,excluded.duration)", params![session,id,duration]).map_err(|e| e.to_string())?;
        for (ordinal, m) in messages.iter().take(500).enumerate() {
            tx.execute(
                "INSERT INTO messages(chat_id,speaker,text,session_id,ordinal) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(session_id,ordinal) DO UPDATE SET speaker=excluded.speaker,text=excluded.text",
                params![id, m.speaker, m.text, session, ordinal as i64],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(title) = messages
            .iter()
            .find(|m| m.speaker == "user" && m.text.trim().chars().count() >= 12)
        {
            let title: String = title.text.trim().chars().take(72).collect();
            tx.execute("UPDATE chats SET title=?1 WHERE id=?2 AND owner=?3 AND custom=0 AND title='New conversation'", params![title,id,owner]).map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
}

fn owner_revision(db: &Connection, owner: &str) -> Result<i64, String> {
    db.query_row(
        "SELECT revision FROM owner_revisions WHERE owner=?1",
        [owner],
        |row| row.get(0),
    )
    .optional()
    .map(|value| value.unwrap_or(0))
    .map_err(|e| e.to_string())
}

fn migrate_messages(db: &Connection) -> Result<(), String> {
    let mut statement = db
        .prepare("PRAGMA table_info(messages)")
        .map_err(|e| e.to_string())?;
    let fields = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(statement);
    if !fields.iter().any(|field| field == "session_id") {
        db.execute("ALTER TABLE messages ADD COLUMN session_id TEXT", [])
            .map_err(|e| e.to_string())?;
    }
    if !fields.iter().any(|field| field == "ordinal") {
        db.execute("ALTER TABLE messages ADD COLUMN ordinal INTEGER", [])
            .map_err(|e| e.to_string())?;
    }
    db.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS message_session_ordinal ON messages(session_id,ordinal); CREATE TABLE IF NOT EXISTS chat_sessions (id TEXT PRIMARY KEY, chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE, duration INTEGER NOT NULL);").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_histories_remain_isolated_and_renames_survive_sessions() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let id = store.create("alice", "Spanish").unwrap();
        let msg = Message {
            speaker: "user".into(),
            text: "Tell me about the city".into(),
        };
        store
            .append_session("alice", &id, std::slice::from_ref(&msg), 13)
            .unwrap();
        assert_eq!(store.list("alice").unwrap()[0].duration_seconds, 13);
        assert!(store.list("bob").unwrap().is_empty());
        assert!(store.messages("bob", &id).unwrap().is_none());
        assert!(!store.rename("bob", &id, "stolen").unwrap());
        assert!(!store.delete("bob", &id).unwrap());
        assert!(store
            .append_session("bob", &id, std::slice::from_ref(&msg), 1)
            .is_err());
        assert!(store.rename("alice", &id, "A favorite city").unwrap());
        store.append_session("alice", &id, &[msg], 2).unwrap();
        assert_eq!(store.list("alice").unwrap()[0].title, "A favorite city");
        assert!(store.delete("alice", &id).unwrap());
        assert!(store.messages("alice", &id).unwrap().is_none());
    }

    #[test]
    fn randomized_owners_cannot_read_each_others_chats() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        for index in 0..250 {
            let owner = format!("owner-{index}");
            let id = store.create(&owner, "English").unwrap();
            assert!(store
                .messages(&format!("other-{index}"), &id)
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn checkpoints_update_existing_messages_without_double_counting_time() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let id = store.create("alice", "Spanish").unwrap();
        let mut messages = vec![Message {
            speaker: "user".into(),
            text: "Partial".into(),
        }];
        store
            .checkpoint("alice", &id, "session", &messages, 5)
            .unwrap();
        messages[0].text = "Complete".into();
        messages.push(Message {
            speaker: "partner".into(),
            text: "Hello".into(),
        });
        store
            .checkpoint("alice", &id, "session", &messages, 12)
            .unwrap();
        assert_eq!(store.messages("alice", &id).unwrap().unwrap(), messages);
        assert_eq!(store.list("alice").unwrap()[0].duration_seconds, 12);
        assert!(store
            .checkpoint("bob", &id, "session", &messages, 15)
            .is_err());
    }

    #[test]
    fn histories_can_be_paged_and_searched_without_crossing_owners() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        for index in 0..125 {
            let id = store.create("alice", "Spanish").unwrap();
            store
                .rename("alice", &id, &format!("Topic {index:03}"))
                .unwrap();
        }
        let (first, more, list_revision) = store.list_page("alice", 0, "topic", "title").unwrap();
        assert_eq!(first.len(), 100);
        assert!(more);
        assert_eq!(
            store
                .list_page("alice", 100, "topic", "title")
                .unwrap()
                .0
                .len(),
            25
        );
        assert!(store
            .list_page("bob", 0, "topic", "title")
            .unwrap()
            .0
            .is_empty());
        let id = &first[0].id;
        let messages: Vec<Message> = (0..125)
            .map(|index| Message {
                speaker: "user".into(),
                text: format!("message-{index}"),
            })
            .collect();
        store
            .checkpoint("alice", id, "session", &messages, 1)
            .unwrap();
        let (recent, has_more, message_revision) =
            store.message_page("alice", id, 0).unwrap().unwrap();
        assert!(has_more);
        assert!(message_revision > list_revision);
        assert_eq!(recent[0].text, "message-25");
        assert_eq!(
            store
                .message_page("alice", id, 100)
                .unwrap()
                .unwrap()
                .0
                .len(),
            25
        );
        assert!(store.message_page("bob", id, 0).unwrap().is_none());
        store
            .checkpoint("alice", id, "session", &messages, 2)
            .unwrap();
        assert!(store.message_page("alice", id, 100).unwrap().unwrap().2 > message_revision);
    }
}
