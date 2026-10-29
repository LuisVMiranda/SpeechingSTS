struct Word<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

fn words(text: &str) -> Vec<Word<'_>> {
    let mut words = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices() {
        if character.is_alphabetic() || matches!(character, '\'' | '’' | '-') {
            start.get_or_insert(index);
        } else if let Some(begin) = start.take() {
            words.push(Word {
                text: &text[begin..index],
                start: begin,
                end: index,
            });
        }
    }
    if let Some(begin) = start {
        words.push(Word {
            text: &text[begin..],
            start: begin,
            end: text.len(),
        });
    }
    words
}

const INTRODUCTIONS: &[&[&str]] = &[
    &["my", "name", "is"],
    &["my", "name's"],
    &["me", "llamo"],
    &["mi", "nombre", "es"],
    &["puedes", "llamarme"],
    &["me", "chamo"],
    &["meu", "nome", "é"],
    &["je", "m'appelle"],
    &["mon", "nom", "est"],
    &["ich", "heiße"],
    &["mein", "name", "ist"],
    &["mi", "chiamo"],
];

const SHORT_INTRODUCTIONS: &[&[&str]] = &[
    &["you", "can", "call", "me"],
    &["call", "me"],
    &["pode", "me", "chamar", "de"],
    &["i'm"],
    &["i’m"],
    &["i", "am"],
    &["soy"],
    &["eu", "sou"],
    &["sou"],
];

fn matches_intro(text: &str, words: &[Word<'_>], start: usize, pattern: &[&str]) -> bool {
    let Some(group) = words.get(start..start + pattern.len()) else {
        return false;
    };
    if group
        .iter()
        .zip(pattern)
        .any(|(word, expected)| word.text.to_lowercase() != *expected)
    {
        return false;
    }
    if text[..group[0].start]
        .chars()
        .next_back()
        .is_some_and(|c| matches!(c, '"' | '“' | '‘'))
    {
        return false;
    }
    group.windows(2).all(|pair| {
        text[pair[0].end..pair[1].start]
            .chars()
            .all(char::is_whitespace)
    })
}

fn candidate_name(word: &str) -> Option<String> {
    let lower = word.to_lowercase();
    let blocked = [
        "a",
        "an",
        "the",
        "not",
        "no",
        "unknown",
        "un",
        "una",
        "no",
        "não",
        "um",
        "uma",
        "o",
        "la",
        "el",
        "mystery",
        "secret",
        "difficult",
        "ai",
        "i",
        "de",
        "from",
    ];
    if word.chars().count() > 32 || blocked.contains(&lower.as_str()) {
        return None;
    }
    let mut chars = word.chars();
    let first = chars.next()?;
    if !first.is_alphabetic() || !word.chars().last()?.is_alphabetic() {
        return None;
    }
    let name: String = first.to_uppercase().chain(chars).collect();
    (name.chars().count() <= 32).then_some(name)
}

/// Recognize explicit self-introductions, leaving incomplete streamed words pending.
pub(super) fn introduced_name(text: &str, completed: bool) -> Option<String> {
    let tokens = words(text);
    let mut found = None;
    for start in 0..tokens.len() {
        for pattern in INTRODUCTIONS.iter().chain(SHORT_INTRODUCTIONS) {
            if !matches_intro(text, &tokens, start, pattern) {
                continue;
            }
            let Some(candidate) = tokens.get(start + pattern.len()) else {
                continue;
            };
            if !completed && candidate.end == text.len() {
                continue;
            }
            if SHORT_INTRODUCTIONS.contains(pattern)
                && !candidate
                    .text
                    .chars()
                    .next()
                    .is_some_and(char::is_uppercase)
            {
                continue;
            }
            if let Some(name) = candidate_name(candidate.text) {
                found = Some(name);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::introduced_name;

    #[test]
    fn recognizes_explicit_introductions_in_three_languages() {
        for (text, expected) in [
            ("Hello, my name is Anna.", "Anna"),
            ("You can call me Leo!", "Leo"),
            ("Hola, me llamo Lucía.", "Lucía"),
            ("Mi nombre es Carlos.", "Carlos"),
            ("Olá, meu nome é Júlia.", "Júlia"),
            ("Pode me chamar de João.", "João"),
            ("I'm Anna.", "Anna"),
            ("Soy María.", "María"),
            ("Eu sou Bruno.", "Bruno"),
            ("Je m'appelle Léa.", "Léa"),
        ] {
            assert_eq!(introduced_name(text, false).as_deref(), Some(expected));
        }
    }

    #[test]
    fn waits_for_a_complete_name_and_ignores_ambiguous_statements() {
        assert_eq!(introduced_name("My name is An", false), None);
        assert_eq!(
            introduced_name("My name is Ana", true).as_deref(),
            Some("Ana")
        );
        for text in [
            "What is your name?",
            "I am happy to help.",
            "I'm from Brazil.",
            "You can call me later.",
            "Call me when you arrive.",
            "Pode me chamar de volta.",
            "Soy una IA.",
            "My name is not Ana.",
            "My name is a mystery.",
            "Say ‘my name is Ana’.",
        ] {
            assert_eq!(introduced_name(text, true), None, "{text}");
        }
    }

    #[test]
    fn candidate_names_stay_bounded_for_many_generated_inputs() {
        let alphabet = ['a', 'Z', 'é', 'ñ', '-', '\'', ' ', '.', '1', '☕'];
        let mut seed = 0x5eed_u64;
        for length in 0..80 {
            for _ in 0..20 {
                let mut candidate = String::new();
                for _ in 0..length {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    candidate.push(alphabet[(seed as usize) % alphabet.len()]);
                }
                let input = format!("My name is {candidate}");
                if let Some(name) = introduced_name(&input, true) {
                    assert!(name.chars().count() <= 32);
                    assert!(name.chars().next().is_some_and(char::is_alphabetic));
                    assert!(name.chars().last().is_some_and(char::is_alphabetic));
                }
            }
        }
    }
}
