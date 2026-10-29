use super::interface_helpers::chat_title;
use super::library_view::SortOrder;
use super::*;
use std::cmp::Reverse;

#[derive(Default)]
pub(super) struct LibrarySearch {
    key: Option<(String, UiLanguage, SortOrder)>,
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
        order: SortOrder,
    ) -> &'a [usize] {
        let key = (crate::languages::fold(query), language, order);
        if self.key.as_ref() != Some(&key) {
            self.matches = library
                .iter()
                .enumerate()
                .filter_map(|(index, chat)| chat_matches(chat, &key.0, language).then_some(index))
                .collect();
            sort_matches(&mut self.matches, library, language, order);
            self.key = Some(key);
        }
        &self.matches
    }
}

fn sort_matches(
    matches: &mut [usize],
    library: &[ChatRecord],
    language: UiLanguage,
    order: SortOrder,
) {
    match order {
        SortOrder::Newest => matches.sort_by(|&a, &b| {
            library[b]
                .updated_at
                .cmp(&library[a].updated_at)
                .then_with(|| library[b].id.cmp(&library[a].id))
        }),
        SortOrder::Oldest => matches.sort_by(|&a, &b| {
            library[a]
                .updated_at
                .cmp(&library[b].updated_at)
                .then_with(|| library[a].id.cmp(&library[b].id))
        }),
        SortOrder::TitleAscending => matches.sort_by_cached_key(|&i| {
            (
                crate::languages::fold(chat_title(&library[i], language)),
                Reverse(library[i].updated_at),
                Reverse(library[i].id.clone()),
            )
        }),
        SortOrder::TitleDescending => matches.sort_by_cached_key(|&i| {
            (
                Reverse(crate::languages::fold(chat_title(&library[i], language))),
                Reverse(library[i].updated_at),
                Reverse(library[i].id.clone()),
            )
        }),
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
        assert_eq!(
            search.matching(&library, "cafe", UiLanguage::English, SortOrder::Newest),
            &[0]
        );
        assert!(search
            .matching(&library, "other", UiLanguage::English, SortOrder::Newest)
            .is_empty());
        assert_eq!(
            search.matching(&library, "español", UiLanguage::Spanish, SortOrder::Newest),
            &[0]
        );
        library.push(ChatRecord::new("Spanish".into()));
        search.invalidate();
        assert_eq!(
            search.matching(
                &library,
                "español",
                UiLanguage::Spanish,
                SortOrder::TitleAscending
            ),
            &[0, 1]
        );
    }

    #[test]
    fn sort_order_reorders_cached_matches_by_date_and_title() {
        let mut chats = Vec::new();
        for (title, updated_at) in [("Zebra", 20), ("Café", 30), ("Apple", 10)] {
            let mut chat = ChatRecord::new("Spanish".into());
            chat.title = title.into();
            chat.updated_at = updated_at;
            chats.push(chat);
        }
        let mut search = LibrarySearch::default();
        let lang = UiLanguage::English;
        assert_eq!(
            search.matching(&chats, "", lang, SortOrder::Newest),
            &[1, 0, 2]
        );
        assert_eq!(
            search.matching(&chats, "", lang, SortOrder::Oldest),
            &[2, 0, 1]
        );
        assert_eq!(
            search.matching(&chats, "", lang, SortOrder::TitleAscending),
            &[2, 1, 0]
        );
        assert_eq!(
            search.matching(&chats, "", lang, SortOrder::TitleDescending),
            &[0, 1, 2]
        );
        assert_eq!(
            search.matching(&chats, "cafe", lang, SortOrder::Oldest),
            &[1]
        );
    }
}
