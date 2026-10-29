use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

pub const GEMINI_URL: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
pub const OPENAI_URL: &str = "wss://api.openai.com/v1/realtime";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Gemini,
    OpenAi,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputMode {
    #[default]
    VoiceActivated,
    PushToTalk,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcut {
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}
impl Default for Shortcut {
    fn default() -> Self {
        Self {
            key: "Space".into(),
            ctrl: false,
            alt: false,
            shift: false,
        }
    }
}
impl Shortcut {
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(&self.key);
        parts.join(" + ")
    }
    pub fn key(&self) -> eframe::egui::Key {
        eframe::egui::Key::from_name(&self.key).unwrap_or(eframe::egui::Key::Space)
    }
    pub fn matches(&self, modifiers: eframe::egui::Modifiers) -> bool {
        self.ctrl == modifiers.ctrl && self.alt == modifiers.alt && self.shift == modifiers.shift
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowPositions {
    pub settings: Option<[f32; 2]>,
    pub log: Option<[f32; 2]>,
}
impl WindowPositions {
    fn path() -> Option<PathBuf> {
        settings_path().map(|path| path.with_file_name("window-positions.json"))
    }
    pub fn load() -> Self {
        Self::path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("Could not locate the settings folder.")?;
        fs::create_dir_all(path.parent().ok_or("Invalid settings path.")?)
            .map_err(|e| e.to_string())?;
        fs::write(
            path,
            serde_json::to_string_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u8,
    pub provider: Provider,
    pub gemini_url: String,
    pub gemini_model: String,
    pub gemini_voice: String,
    pub openai_url: String,
    pub openai_model: String,
    pub openai_voice: String,
    pub language: String,
    pub level: String,
    pub speed: f32,
    pub fade_seconds: f32,
    pub minimal: bool,
    pub instructions: String,
    pub input_mode: InputMode,
    pub noise_gate_enabled: bool,
    pub noise_gate_db: f32,
    pub theme: Theme,
    pub push_to_talk_shortcut: Shortcut,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 4,
            provider: Provider::Gemini,
            gemini_url: GEMINI_URL.into(),
            gemini_model: "gemini-3.8-live".into(),
            gemini_voice: "Aoede".into(),
            openai_url: OPENAI_URL.into(),
            openai_model: "gpt-realtime-2.1".into(),
            openai_voice: "marin".into(),
            language: "English".into(),
            level: "Intermediate".into(),
            speed: 1.0,
            fade_seconds: 8.0,
            minimal: false,
            instructions: String::new(),
            input_mode: InputMode::VoiceActivated,
            noise_gate_enabled: true,
            noise_gate_db: -42.0,
            theme: Theme::Light,
            push_to_talk_shortcut: Shortcut::default(),
        }
    }
}
impl Settings {
    pub fn endpoint(&self) -> &str {
        match self.provider {
            Provider::Gemini => &self.gemini_url,
            Provider::OpenAi => &self.openai_url,
        }
    }
    pub fn model(&self) -> &str {
        match self.provider {
            Provider::Gemini => &self.gemini_model,
            Provider::OpenAi => &self.openai_model,
        }
    }
    pub fn voice(&self) -> &str {
        match self.provider {
            Provider::Gemini => &self.gemini_voice,
            Provider::OpenAi => &self.openai_voice,
        }
    }
    pub fn load() -> Self {
        let mut value = settings_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| Self::from_persisted(&s))
            .unwrap_or_default();
        value.speed = value.speed.clamp(0.7, 1.4);
        value.fade_seconds = value.fade_seconds.clamp(2.0, 30.0);
        value.noise_gate_db = value.noise_gate_db.clamp(-65.0, -20.0);
        value
    }
    fn from_persisted(text: &str) -> Option<Self> {
        let mut data: serde_json::Value = serde_json::from_str(text).ok()?;
        if data.get("schema_version").is_none()
            && data.get("fade_seconds").and_then(|v| v.as_f64()) == Some(6.0)
        {
            data["fade_seconds"] = serde_json::json!(8.0);
        }
        if data
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            < 4
        {
            if data
                .get("speed")
                .and_then(|v| v.as_f64())
                .is_some_and(|speed| (speed - 0.9).abs() < 0.001)
            {
                data["speed"] = serde_json::json!(0.82);
            }
            data["schema_version"] = serde_json::json!(4);
        }
        serde_json::from_value(data).ok()
    }
    pub fn save(&self) -> Result<(), String> {
        if crate::languages::canonical(self.provider, &self.language).is_none() {
            return Err("Choose a supported language from the suggestions.".into());
        }
        let path = settings_path().ok_or("Could not locate the settings folder.")?;
        fs::create_dir_all(path.parent().ok_or("Invalid settings path.")?)
            .map_err(|e| e.to_string())?;
        fs::write(
            path,
            serde_json::to_string_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
}
fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("Speeching").join("settings.json"))
}

pub const GEMINI_VOICES: &[(&str, &str)] = &[
    ("Aoede", "Breezy"),
    ("Puck", "Upbeat"),
    ("Kore", "Firm"),
    ("Zephyr", "Bright"),
    ("Charon", "Informative"),
    ("Fenrir", "Excitable"),
    ("Leda", "Youthful"),
    ("Orus", "Firm"),
    ("Callirrhoe", "Easy-going"),
    ("Autonoe", "Bright"),
    ("Enceladus", "Breathy"),
    ("Iapetus", "Clear"),
    ("Umbriel", "Easy-going"),
    ("Algieba", "Smooth"),
    ("Despina", "Smooth"),
    ("Erinome", "Clear"),
    ("Algenib", "Gravelly"),
    ("Rasalgethi", "Informative"),
    ("Laomedeia", "Upbeat"),
    ("Achernar", "Soft"),
    ("Alnilam", "Firm"),
    ("Schedar", "Even"),
    ("Gacrux", "Mature"),
    ("Pulcherrima", "Forward"),
    ("Achird", "Friendly"),
    ("Zubenelgenubi", "Casual"),
    ("Vindemiatrix", "Gentle"),
    ("Sadachbia", "Lively"),
    ("Sadaltager", "Knowledgeable"),
    ("Sulafat", "Warm"),
];
pub const OPENAI_VOICES: &[&str] = &[
    "alloy", "ash", "ballad", "coral", "echo", "sage", "shimmer", "verse", "marin", "cedar",
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serialized_settings_never_contain_an_api_key() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!json.contains("api_key"));
        assert_eq!(
            serde_json::from_str::<Settings>(&json)
                .unwrap()
                .fade_seconds,
            8.0
        );
    }
    #[test]
    fn old_default_fade_is_upgraded_without_resetting_other_preferences() {
        let settings =
            Settings::from_persisted(r#"{"fade_seconds":6.0,"speed":0.9,"language":"Spanish"}"#)
                .unwrap();
        assert_eq!(settings.fade_seconds, 8.0);
        assert_eq!(settings.speed, 0.82);
        assert_eq!(settings.language, "Spanish");
        assert_eq!(
            Settings::from_persisted(r#"{"fade_seconds":12.0}"#)
                .unwrap()
                .fade_seconds,
            12.0
        );
        assert_eq!(
            Settings::from_persisted(r#"{"schema_version":3,"fade_seconds":6.0}"#)
                .unwrap()
                .fade_seconds,
            6.0
        );
    }
    #[test]
    fn theme_shortcut_and_window_positions_round_trip() {
        let mut settings = Settings::default();
        settings.theme = Theme::Dark;
        settings.push_to_talk_shortcut = Shortcut {
            key: "K".into(),
            ctrl: true,
            alt: false,
            shift: true,
        };
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.theme, Theme::Dark);
        assert_eq!(restored.push_to_talk_shortcut.label(), "Ctrl + Shift + K");
        assert_eq!(restored.push_to_talk_shortcut.key(), eframe::egui::Key::K);
        assert!(restored
            .push_to_talk_shortcut
            .matches(eframe::egui::Modifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            }));
        assert!(!restored
            .push_to_talk_shortcut
            .matches(eframe::egui::Modifiers {
                ctrl: true,
                ..Default::default()
            }));
        let positions = WindowPositions {
            settings: Some([-320.0, 180.0]),
            log: Some([640.0, 250.0]),
        };
        let restored: WindowPositions =
            serde_json::from_str(&serde_json::to_string(&positions).unwrap()).unwrap();
        assert_eq!(restored, positions);
    }
}
