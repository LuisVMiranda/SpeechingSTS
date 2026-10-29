use super::{SavedSession, Speaker};

const IGNORED_WORDS: &[&str] = &[
    "a",
    "about",
    "am",
    "and",
    "are",
    "be",
    "been",
    "but",
    "can",
    "could",
    "do",
    "for",
    "going",
    "good",
    "great",
    "hello",
    "hey",
    "hi",
    "how",
    "i",
    "im",
    "is",
    "it",
    "just",
    "know",
    "language",
    "learn",
    "learning",
    "lets",
    "like",
    "my",
    "name",
    "nice",
    "no",
    "okay",
    "please",
    "practice",
    "practicing",
    "really",
    "say",
    "so",
    "some",
    "speak",
    "talk",
    "tell",
    "thank",
    "thanks",
    "that",
    "the",
    "there",
    "this",
    "to",
    "today",
    "want",
    "was",
    "we",
    "well",
    "what",
    "when",
    "with",
    "would",
    "yes",
    "you",
    "your",
    "all",
    "right",
    "doing",
    "fine",
    "ok",
    "sure",
    "yeah",
    "yep",
    "english",
    "help",
    "spanish",
    "portuguese",
    "conversation",
    "aqui",
    "bem",
    "bien",
    "bom",
    "boa",
    "como",
    "com",
    "con",
    "de",
    "del",
    "do",
    "dos",
    "el",
    "en",
    "es",
    "esta",
    "estamos",
    "estoy",
    "estas",
    "espanol",
    "ingles",
    "portugues",
    "conversacion",
    "conversa",
    "conversar",
    "aprender",
    "ayudar",
    "ajudar",
    "ayudarte",
    "ayudarme",
    "puedo",
    "puedes",
    "posso",
    "gusta",
    "gosto",
    "beleza",
    "bueno",
    "certo",
    "claro",
    "legal",
    "perfecto",
    "vale",
    "tu",
    "eu",
    "falar",
    "gostaria",
    "gracias",
    "hablar",
    "hola",
    "idioma",
    "la",
    "las",
    "los",
    "me",
    "muito",
    "muy",
    "na",
    "nao",
    "nos",
    "o",
    "obrigada",
    "obrigado",
    "oi",
    "ola",
    "os",
    "para",
    "por",
    "praticar",
    "practicar",
    "que",
    "quero",
    "quiero",
    "se",
    "si",
    "sim",
    "sobre",
    "sou",
    "soy",
    "tal",
    "te",
    "tengo",
    "tudo",
    "um",
    "uma",
    "un",
    "una",
    "vamos",
    "voce",
    "yo",
    "bonjour",
    "salut",
    "merci",
    "hallo",
    "ciao",
    "你好",
    "こんにちは",
    "안녕하세요",
    "مرحبا",
];

const TOPIC_PREFIXES: &[&[&str]] = &[
    &["i", "would", "like", "to", "talk", "about"],
    &["i", "would", "like", "to", "discuss"],
    &["id", "like", "to", "talk", "about"],
    &["id", "like", "to", "discuss"],
    &["i", "want", "to", "talk", "about"],
    &["can", "we", "talk", "about"],
    &["lets", "talk", "about"],
    &["what", "do", "you", "think", "about"],
    &["me", "gustaria", "aprender", "sobre"],
    &["me", "gustaria", "hablar", "sobre"],
    &["me", "gustaria", "hablar", "de"],
    &["quiero", "hablar", "sobre"],
    &["quiero", "hablar", "de"],
    &["vamos", "a", "hablar", "sobre"],
    &["vamos", "a", "hablar", "de"],
    &["podemos", "hablar", "sobre"],
    &["podemos", "hablar", "de"],
    &["hablemos", "sobre"],
    &["hablemos", "de"],
    &["eu", "gostaria", "de", "conversar", "sobre"],
    &["eu", "gostaria", "de", "falar", "sobre"],
    &["gostaria", "de", "conversar", "sobre"],
    &["gostaria", "de", "falar", "sobre"],
    &["eu", "quero", "falar", "sobre"],
    &["quero", "falar", "sobre"],
    &["quero", "aprender", "sobre"],
    &["vamos", "conversar", "sobre"],
    &["vamos", "falar", "sobre"],
    &["podemos", "falar", "sobre"],
];

pub(super) fn automatic_title(sessions: &[SavedSession]) -> String {
    let messages: Vec<_> = sessions
        .iter()
        .rev()
        .flat_map(|session| session.messages.iter().rev())
        .take(120)
        .collect();
    let user = best_title(&messages, Speaker::User);
    let partner = || best_title(&messages, Speaker::Partner);
    user.or_else(partner).unwrap_or_else(|| {
        if sessions.is_empty() {
            "New conversation".into()
        } else {
            "Practice conversation".into()
        }
    })
}

fn best_title(messages: &[&super::MemoryMessage], speaker: Speaker) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for message in messages
        .iter()
        .rev()
        .filter(|message| message.speaker == speaker)
    {
        let excerpt: String = message.text.chars().take(1_000).collect();
        for sentence in excerpt.split(['.', '!', '?', '。', '！', '？', '\n']) {
            let cleaned = clean_sentence(sentence);
            let sentence = topic_phrase(without_introduction(&cleaned));
            let score = score_sentence(&sentence);
            if score > 0 && best.as_ref().is_none_or(|(current, _)| score >= *current) {
                best = Some((score, sentence));
            }
        }
    }
    best.map(|(_, title)| shorten(&title))
}

fn without_introduction(sentence: &str) -> &str {
    let normalized = crate::languages::fold(sentence);
    let first_clause = normalized.split(',').next().unwrap_or_default().trim();
    let explicit = [
        "my name is ",
        "my name's ",
        "i am called ",
        "i'm called ",
        "me llamo ",
        "mi nombre es ",
        "me chamo ",
        "meu nome e ",
        "je m'appelle ",
        "mi chiamo ",
        "ich heisse ",
    ]
    .iter()
    .any(|prefix| first_clause.starts_with(prefix));
    let short_name = ["soy ", "sou ", "i am ", "i'm "]
        .iter()
        .any(|prefix| first_clause.starts_with(prefix))
        && first_clause.split_whitespace().count() <= 3;
    if !explicit && !short_name {
        return sentence;
    }
    sentence.split_once(',').map_or("", |(_, rest)| rest.trim())
}

fn topic_phrase(sentence: &str) -> String {
    let words: Vec<_> = sentence.split_whitespace().collect();
    let normalized: Vec<_> = words
        .iter()
        .map(|word| crate::languages::fold(word).replace(['\'', '’'], ""))
        .collect();
    for prefix in TOPIC_PREFIXES {
        if words.len() <= prefix.len()
            || !normalized
                .iter()
                .zip(*prefix)
                .all(|(word, part)| word == part)
        {
            continue;
        }
        let topic = words[prefix.len()..].join(" ");
        if score_sentence(&topic) > 0 {
            let mut chars = topic.chars();
            return chars.next().unwrap().to_uppercase().chain(chars).collect();
        }
    }
    sentence.to_owned()
}

fn clean_sentence(sentence: &str) -> String {
    let sentence = sentence.trim_matches(|ch: char| ch.is_whitespace() || "¿¡,;:—-".contains(ch));
    let sentence = match sentence.split_once(',') {
        Some((greeting, rest)) if is_greeting(greeting) => rest,
        _ => sentence,
    };
    sentence.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_greeting(value: &str) -> bool {
    matches!(
        crate::languages::fold(value).as_str(),
        "hi" | "hello" | "hey" | "hola" | "oi" | "ola" | "bonjour" | "salut" | "hallo" | "ciao"
    )
}

fn score_sentence(sentence: &str) -> usize {
    let folded = crate::languages::fold(sentence);
    let words: Vec<_> = folded
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let useful = words
        .iter()
        .filter(|word| word.chars().count() > 1 && !IGNORED_WORDS.contains(word))
        .count();
    if useful == 0 {
        return 0;
    }
    useful.min(8) * 3 + words.len().min(12)
}

fn shorten(value: &str) -> String {
    let mut chars = value.chars();
    let title: String = chars.by_ref().take(60).collect();
    if chars.next().is_some() {
        format!("{}…", title.trim_end())
    } else {
        title
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::MemoryMessage;

    fn session(messages: &[(Speaker, &str)]) -> SavedSession {
        SavedSession {
            voice: "voice".into(),
            duration_seconds: 0,
            messages: messages
                .iter()
                .map(|(speaker, text)| MemoryMessage {
                    speaker: *speaker,
                    text: (*text).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn skips_greetings_and_updates_when_a_topic_emerges() {
        let mut chat = session(&[(Speaker::User, "Hola. Muy bien, gracias. ¿Y tú?")]);
        assert_eq!(automatic_title(&[chat.clone()]), "Practice conversation");
        chat.messages.push(MemoryMessage {
            speaker: Speaker::User,
            text: "Me gustaría aprender sobre recetas italianas.".into(),
        });
        assert_eq!(automatic_title(&[chat]), "Recetas italianas");
    }

    #[test]
    fn uses_the_partner_only_if_the_learner_has_no_descriptive_phrase() {
        let chat = session(&[
            (Speaker::User, "Oi!"),
            (Speaker::Partner, "Vamos conversar sobre viagens ao Japão?"),
        ]);
        assert_eq!(automatic_title(&[chat]), "Viagens ao Japão");
    }

    #[test]
    fn shortens_common_introductions_without_inventing_a_topic() {
        let chat = session(&[(
            Speaker::User,
            "I'd like to discuss Japanese food and traditions.",
        )]);
        assert_eq!(automatic_title(&[chat]), "Japanese food and traditions");
        let chat = session(&[(Speaker::User, "Quiero hablar de recetas italianas.")]);
        assert_eq!(automatic_title(&[chat]), "Recetas italianas");
    }

    #[test]
    fn self_introductions_do_not_become_topics() {
        let chat = session(&[
            (Speaker::User, "Hola. Me llamo Paula."),
            (
                Speaker::Partner,
                "Hola, soy Ana. ¿En qué puedo ayudarte a practicar español?",
            ),
        ]);
        assert_eq!(automatic_title(&[chat]), "Practice conversation");
        let chat = session(&[
            (Speaker::User, "Oi!"),
            (
                Speaker::Partner,
                "Oi, sou Ana, vamos conversar sobre viagens ao Japão.",
            ),
        ]);
        assert_eq!(automatic_title(&[chat]), "Viagens ao Japão");
    }

    #[test]
    fn titles_are_unicode_safe_and_bounded() {
        let message = "旅行".repeat(80);
        let chat = session(&[(Speaker::User, &message)]);
        let title = automatic_title(&[chat]);
        assert_eq!(title.chars().count(), 61);
        assert!(title.ends_with('…'));
        assert_eq!(automatic_title(&[]), "New conversation");
    }
}
