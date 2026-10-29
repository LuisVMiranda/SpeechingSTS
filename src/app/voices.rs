use super::*;

pub(super) fn voice_symbol(provider: Provider, voice: &str) -> &'static str {
    let feminine = match provider {
        Provider::Gemini => matches!(
            voice,
            "Achernar"
                | "Aoede"
                | "Autonoe"
                | "Callirrhoe"
                | "Despina"
                | "Erinome"
                | "Gacrux"
                | "Kore"
                | "Laomedeia"
                | "Leda"
                | "Pulcherrima"
                | "Sulafat"
                | "Vindemiatrix"
                | "Zephyr"
        ),
        Provider::OpenAi => matches!(voice, "alloy" | "coral" | "sage" | "shimmer" | "marin"),
    };
    if feminine {
        "♀"
    } else {
        "♂"
    }
}

pub(super) fn voice_description<'a>(language: UiLanguage, description: &'a str) -> &'a str {
    if description == "Clear" && language != UiLanguage::English {
        language.text("Clear voice")
    } else {
        language.text(description)
    }
}

pub(super) fn voice_label(provider: Provider, voice: &str, language: UiLanguage) -> String {
    let symbol = voice_symbol(provider, voice);
    let register = language.text(if symbol == "♀" {
        "Feminine"
    } else {
        "Masculine"
    });
    let profile = match provider {
        Provider::Gemini => GEMINI_VOICES
            .iter()
            .position(|(name, _)| *name == voice)
            .map(|index| {
                (
                    index + 1,
                    voice_description(language, GEMINI_VOICES[index].1),
                )
            }),
        Provider::OpenAi => OPENAI_VOICES
            .iter()
            .position(|name| *name == voice)
            .map(|index| (index + 1, language.text("Tone varies"))),
    };
    match profile {
        Some((index, tone)) => format!("{symbol} {register} · {tone} · {index:02}"),
        None => language.text("Custom voice").to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn voice_labels_are_distinct_localized_and_do_not_use_provider_names() {
        for language in UiLanguage::ALL {
            for provider in [Provider::Gemini, Provider::OpenAi] {
                let names: Vec<&str> = match provider {
                    Provider::Gemini => GEMINI_VOICES.iter().map(|(name, _)| *name).collect(),
                    Provider::OpenAi => OPENAI_VOICES.to_vec(),
                };
                let labels: HashSet<_> = names
                    .iter()
                    .map(|name| voice_label(provider, name, language))
                    .collect();
                assert_eq!(labels.len(), names.len());
                for (name, label) in names.iter().zip(
                    names
                        .iter()
                        .map(|name| voice_label(provider, name, language)),
                ) {
                    assert!(!label.to_lowercase().contains(&name.to_lowercase()));
                }
            }
        }
    }
}
