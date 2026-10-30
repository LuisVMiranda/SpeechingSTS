const MESSAGES: &str = include_str!("../launcher-i18n.tsv");

pub fn text(key: &str) -> &'static str {
    let language = std::env::var("SPEECHING_LAUNCHER_LANGUAGE").unwrap_or_default();
    lookup(&language, key).unwrap_or(key_fallback())
}

fn key_fallback() -> &'static str {
    "A launcher message is unavailable."
}

fn lookup(language: &str, key: &str) -> Option<&'static str> {
    let column = match language.to_ascii_lowercase().as_str() {
        "es" => 2,
        "pt" => 3,
        _ => 1,
    };
    MESSAGES.lines().find_map(|line| {
        let mut fields = line.split('\t');
        (fields.next()? == key)
            .then(|| fields.nth(column - 1))
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn launcher_messages_have_three_translations_and_unique_keys() {
        let mut keys = HashSet::new();
        for line in MESSAGES.lines() {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 4, "invalid launcher message: {line}");
            assert!(fields.iter().all(|field| !field.is_empty()));
            assert!(
                keys.insert(fields[0]),
                "duplicate launcher message: {}",
                fields[0]
            );
        }
        assert_eq!(lookup("es", "cli_saved"), Some("Cuenta guardada: {0}"));
        assert_eq!(lookup("PT", "cli_saved"), Some("Conta salva: {0}"));
        assert_eq!(lookup("unknown", "cli_saved"), Some("Account saved: {0}"));
    }
}
