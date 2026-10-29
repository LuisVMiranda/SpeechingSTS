use crate::store::Message as Transcript;
const MAX_TEXT: usize = 8192;
const MAX_TRANSCRIPT: usize = 128_000;

pub(super) struct Recorder {
    pub(super) messages: Vec<Transcript>,
    ids: Vec<Option<String>>,
    length: usize,
}
impl Recorder {
    pub(super) fn new() -> Self {
        Self {
            messages: Vec::new(),
            ids: Vec::new(),
            length: 0,
        }
    }
    pub(super) fn add(
        &mut self,
        speaker: &str,
        text: &str,
        id: Option<String>,
        replace: bool,
    ) -> bool {
        if text.len() > MAX_TEXT || self.length + text.len() > MAX_TRANSCRIPT {
            return false;
        }
        let matching = self.matching(speaker, id.as_deref());
        if let Some(index) = matching {
            let last = &mut self.messages[index];
            if replace {
                last.text = text.into();
            } else {
                last.text.push_str(text);
            }
        } else if self.messages.len() < 500 {
            self.messages.push(Transcript {
                speaker: speaker.into(),
                text: text.into(),
            });
            self.ids.push(id);
        } else {
            return false;
        }
        self.length += text.len();
        true
    }

    fn matching(&self, speaker: &str, id: Option<&str>) -> Option<usize> {
        if let Some(id) = id {
            return self.ids.iter().enumerate().position(|(index, previous)| {
                previous.as_deref() == Some(id) && self.messages[index].speaker == speaker
            });
        }
        self.messages
            .len()
            .checked_sub(1)
            .filter(|index| self.ids[*index].is_none() && self.messages[*index].speaker == speaker)
    }

    pub(super) fn reserve_user(&mut self, id: &str) -> bool {
        if self.matching("user", Some(id)).is_some() {
            return true;
        }
        if self.messages.len() >= 500 {
            return false;
        }
        self.messages.push(Transcript {
            speaker: "user".into(),
            text: String::new(),
        });
        self.ids.push(Some(id.to_owned()));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};
    #[test]
    fn transcripts_are_merged_and_bounded() {
        let mut recorder = Recorder::new();
        recorder.add("user", "hello", Some("x".into()), false);
        recorder.add("user", " there", Some("x".into()), false);
        recorder.add("user", "Hello there.", Some("x".into()), true);
        assert_eq!(recorder.messages[0].text, "Hello there.");
        recorder.add("partner", &"x".repeat(MAX_TEXT + 1), None, false);
        assert_eq!(recorder.messages.len(), 1);
    }

    #[test]
    fn late_transcription_completion_updates_the_original_item() {
        let mut recorder = Recorder::new();
        recorder.add("user", "Hel", Some("user-1".into()), false);
        recorder.add("partner", "Hi!", Some("partner-1".into()), false);
        recorder.add("user", "Hello.", Some("user-1".into()), true);
        assert_eq!(recorder.messages.len(), 2);
        assert_eq!(recorder.messages[0].text, "Hello.");
        assert_eq!(recorder.messages[1].text, "Hi!");
    }
    #[test]
    fn asynchronous_first_user_transcript_keeps_turn_order() {
        let mut recorder = Recorder::new();
        assert!(recorder.reserve_user("user-1"));
        assert!(recorder.add("partner", "Answer", Some("partner-1".into()), false));
        assert!(recorder.add("user", "Question", Some("user-1".into()), true));
        assert_eq!(recorder.messages[0].text, "Question");
        assert_eq!(recorder.messages[1].text, "Answer");
    }

    #[test]
    fn seeded_interleavings_update_the_correct_user_items() {
        let mut rng = StdRng::seed_from_u64(0x5eed_2026);
        for _ in 0..100 {
            let mut recorder = Recorder::new();
            let count = rng.gen_range(1..30);
            for index in 0..count {
                assert!(recorder.reserve_user(&format!("user-{index}")));
                assert!(recorder.add(
                    "partner",
                    &format!("answer-{index}"),
                    Some(format!("partner-{index}")),
                    false
                ));
            }
            let mut order: Vec<_> = (0..count).collect();
            order.shuffle(&mut rng);
            for index in order {
                assert!(recorder.add(
                    "user",
                    &format!("question-{index}"),
                    Some(format!("user-{index}")),
                    true
                ));
            }
            for index in 0..count {
                assert_eq!(
                    recorder.messages[index * 2].text,
                    format!("question-{index}")
                );
                assert_eq!(
                    recorder.messages[index * 2 + 1].text,
                    format!("answer-{index}")
                );
            }
        }
    }
}
