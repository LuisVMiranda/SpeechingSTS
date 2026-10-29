use crate::config::Provider;
use crate::i18n::UiLanguage;

// Google Live API capability list, checked September 2026. Generic Chinese and
// Portuguese are useful aliases for the supported regional variants.
const GEMINI: &[&str] = &[
    "Afrikaans",
    "Akan",
    "Albanian",
    "Amharic",
    "Arabic",
    "Armenian",
    "Assamese",
    "Azerbaijani",
    "Basque",
    "Belarusian",
    "Bengali",
    "Bosnian",
    "Bulgarian",
    "Burmese",
    "Catalan",
    "Cebuano",
    "Chinese",
    "Chinese (Simplified)",
    "Chinese (Traditional)",
    "Croatian",
    "Czech",
    "Danish",
    "Dutch",
    "English",
    "Estonian",
    "Faroese",
    "Filipino",
    "Finnish",
    "French",
    "Galician",
    "Georgian",
    "German",
    "Greek",
    "Gujarati",
    "Hausa",
    "Hebrew",
    "Hindi",
    "Hungarian",
    "Icelandic",
    "Indonesian",
    "Irish",
    "Italian",
    "Japanese",
    "Kannada",
    "Kazakh",
    "Khmer",
    "Kinyarwanda",
    "Korean",
    "Kurdish",
    "Kyrgyz",
    "Lao",
    "Latvian",
    "Lithuanian",
    "Macedonian",
    "Malay",
    "Malayalam",
    "Maltese",
    "Maori",
    "Marathi",
    "Mongolian",
    "Nepali",
    "Norwegian",
    "Odia",
    "Oromo",
    "Pashto",
    "Persian",
    "Polish",
    "Portuguese",
    "Portuguese (Brazil)",
    "Portuguese (Portugal)",
    "Punjabi",
    "Quechua",
    "Romanian",
    "Romansh",
    "Russian",
    "Serbian",
    "Sindhi",
    "Sinhala",
    "Slovak",
    "Slovenian",
    "Somali",
    "Southern Sotho",
    "Spanish",
    "Swahili",
    "Swedish",
    "Tajik",
    "Tamil",
    "Telugu",
    "Thai",
    "Tswana",
    "Turkish",
    "Turkmen",
    "Ukrainian",
    "Urdu",
    "Uzbek",
    "Vietnamese",
    "Welsh",
    "Western Frisian",
    "Wolof",
    "Yoruba",
    "Zulu",
];

// Conservative list of languages documented for OpenAI speech output. Actual
// quality and availability can differ for a particular Realtime model.
const OPENAI: &[&str] = &[
    "Afrikaans",
    "Arabic",
    "Armenian",
    "Azerbaijani",
    "Belarusian",
    "Bosnian",
    "Bulgarian",
    "Catalan",
    "Chinese",
    "Croatian",
    "Czech",
    "Danish",
    "Dutch",
    "English",
    "Estonian",
    "Finnish",
    "French",
    "Galician",
    "German",
    "Greek",
    "Hebrew",
    "Hindi",
    "Hungarian",
    "Icelandic",
    "Indonesian",
    "Italian",
    "Japanese",
    "Kannada",
    "Kazakh",
    "Korean",
    "Latvian",
    "Lithuanian",
    "Macedonian",
    "Malay",
    "Marathi",
    "Maori",
    "Nepali",
    "Norwegian",
    "Persian",
    "Polish",
    "Portuguese",
    "Romanian",
    "Russian",
    "Serbian",
    "Slovak",
    "Slovenian",
    "Spanish",
    "Swahili",
    "Swedish",
    "Tagalog",
    "Tamil",
    "Thai",
    "Turkish",
    "Ukrainian",
    "Urdu",
    "Vietnamese",
    "Welsh",
];

pub fn supported(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Gemini => GEMINI,
        Provider::OpenAi => OPENAI,
    }
}

pub fn canonical(provider: Provider, value: &str) -> Option<&'static str> {
    supported(provider)
        .iter()
        .copied()
        .find(|language| language.eq_ignore_ascii_case(value.trim()))
}

#[cfg(test)]
pub fn matching(provider: Provider, query: &str) -> Vec<&'static str> {
    let query = query.trim().to_lowercase();
    supported(provider)
        .iter()
        .copied()
        .filter(|language| language.to_lowercase().contains(&query))
        .collect()
}

pub fn fold(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

pub fn canonical_localized(
    provider: Provider,
    value: &str,
    language: UiLanguage,
) -> Option<&'static str> {
    let query = fold(value);
    supported(provider)
        .iter()
        .copied()
        .find(|name| fold(name) == query || fold(language.text(name)) == query)
}

pub fn matching_localized(
    provider: Provider,
    query: &str,
    language: UiLanguage,
) -> Vec<&'static str> {
    let query = fold(query);
    let mut matches: Vec<_> = supported(provider)
        .iter()
        .copied()
        .filter(|name| fold(name).contains(&query) || fold(language.text(name)).contains(&query))
        .collect();
    matches.sort_by_key(|name| fold(language.text(name)));
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_provider_specific_languages_and_matches_case_insensitively() {
        assert_eq!(
            canonical(Provider::Gemini, " portuguese (brazil) "),
            Some("Portuguese (Brazil)")
        );
        assert_eq!(canonical(Provider::OpenAi, "Portuguese (Brazil)"), None);
        assert_eq!(canonical(Provider::Gemini, "Klingon"), None);
        assert!(matching(Provider::Gemini, "port").contains(&"Portuguese (Brazil)"));
        assert!(matching(Provider::OpenAi, "klingon").is_empty());
    }
    #[test]
    fn localized_search_is_accent_insensitive_and_keeps_canonical_names() {
        assert_eq!(
            canonical_localized(
                Provider::Gemini,
                "portugues (brasil)",
                UiLanguage::PortugueseBrazil
            ),
            Some("Portuguese (Brazil)")
        );
        assert_eq!(
            canonical_localized(Provider::Gemini, "espanol", UiLanguage::Spanish),
            Some("Spanish")
        );
        assert_eq!(
            canonical_localized(
                Provider::OpenAi,
                "português (brasil)",
                UiLanguage::PortugueseBrazil
            ),
            None
        );
        assert!(
            matching_localized(Provider::Gemini, "frances", UiLanguage::Spanish)
                .contains(&"French")
        );
        assert!(
            matching_localized(Provider::Gemini, "english", UiLanguage::PortugueseBrazil)
                .contains(&"English")
        );
        for name in supported(Provider::Gemini)
            .iter()
            .chain(supported(Provider::OpenAi))
        {
            assert!(
                crate::i18n::has_translation(name),
                "missing language: {name}"
            );
        }
    }
}
