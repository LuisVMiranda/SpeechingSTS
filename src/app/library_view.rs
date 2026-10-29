use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum SortOrder {
    Oldest,
    TitleAscending,
    TitleDescending,
    #[default]
    #[serde(other)]
    Newest,
}

impl SortOrder {
    pub(super) const ALL: [Self; 4] = [
        Self::Newest,
        Self::Oldest,
        Self::TitleAscending,
        Self::TitleDescending,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Newest => "Newest first",
            Self::Oldest => "Oldest first",
            Self::TitleAscending => "Title A–Z",
            Self::TitleDescending => "Title Z–A",
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Preference {
    order: SortOrder,
}

pub(super) struct ConversationView {
    pub(super) order: SortOrder,
    path: Option<PathBuf>,
}

impl ConversationView {
    pub(super) fn load() -> Self {
        let path =
            dirs::config_dir().map(|dir| dir.join("Speeching").join("conversation-view.json"));
        Self::from_path(path)
    }

    fn from_path(path: Option<PathBuf>) -> Self {
        let order = path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<Preference>(&text).ok())
            .unwrap_or_default()
            .order;
        Self { order, path }
    }

    #[cfg(test)]
    pub(super) fn for_test(path: PathBuf) -> Self {
        Self::from_path(Some(path))
    }

    pub(super) fn save(&self) -> Result<(), String> {
        let path = self
            .path
            .as_ref()
            .ok_or("Could not locate the settings folder.")?;
        fs::create_dir_all(path.parent().ok_or("Invalid settings path.")?)
            .map_err(|error| error.to_string())?;
        let json = serde_json::to_string(&Preference { order: self.order })
            .map_err(|error| error.to_string())?;
        fs::write(path, json).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_preference_round_trips_and_unknown_values_default_to_newest() {
        for order in SortOrder::ALL {
            let value = Preference { order };
            let restored: Preference =
                serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
            assert_eq!(restored.order, order);
        }
        let restored: Preference = serde_json::from_str(r#"{"order":"Unknown"}"#).unwrap();
        assert_eq!(restored.order, SortOrder::Newest);
    }
}
