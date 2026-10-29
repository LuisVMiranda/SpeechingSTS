use super::interface_helpers::chat_title;
use super::*;

#[derive(Default)]
pub(super) struct LibrarySearch {
    key: Option<(String, UiLanguage)>,
    matches: Vec<usize>,
}

impl LibrarySearch {
    pub(super) fn invalidate(&mut self) {
        self.key = None;
    }

    pub(super) fn matching<'a>(
        &'a mut self,
        library: &[ChatRecord],
        query: &str,
        language: UiLanguage,
    ) -> &'a [usize] {
        let key = (crate::languages::fold(query), language);
        if self.key.as_ref() != Some(&key) {
            self.matches = library
                .iter()
                .enumerate()
                .filter_map(|(index, chat)| chat_matches(chat, &key.0, language).then_some(index))
                .collect();
            self.key = Some(key);
        }
        &self.matches
    }
}

fn chat_matches(chat: &ChatRecord, query: &str, language: UiLanguage) -> bool {
    query.is_empty()
        || crate::languages::fold(chat_title(chat, language)).contains(query)
        || crate::languages::fold(language.text(&chat.language)).contains(query)
        || crate::languages::fold(&chat.language).contains(query)
        || chat.sessions.iter().any(|session| {
            session
                .messages
                .iter()
                .any(|message| crate::languages::fold(&message.text).contains(query))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_search_tracks_query_language_and_library_changes() {
        let mut chat = ChatRecord::new("Spanish".into());
        chat.title = "Café practice".into();
        let mut library = vec![chat];
        let mut search = LibrarySearch::default();
        assert_eq!(search.matching(&library, "cafe", UiLanguage::English), &[0]);
        assert!(search
            .matching(&library, "other", UiLanguage::English)
            .is_empty());
        assert_eq!(
            search.matching(&library, "español", UiLanguage::Spanish),
            &[0]
        );
        library.push(ChatRecord::new("Spanish".into()));
        search.invalidate();
        assert_eq!(
            search.matching(&library, "español", UiLanguage::Spanish),
            &[0, 1]
        );
    }
}
