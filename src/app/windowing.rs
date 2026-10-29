#[derive(Default)]
pub(super) struct WindowPlacement {
    initial: Option<Option<[f32; 2]>>,
}

impl WindowPlacement {
    pub(super) fn position(&mut self, remembered: Option<[f32; 2]>) -> Option<[f32; 2]> {
        *self.initial.get_or_insert(remembered)
    }

    pub(super) fn reset(&mut self) {
        self.initial = None;
    }
}

#[cfg(test)]
mod tests {
    use super::WindowPlacement;
    use eframe::egui::{ViewportBuilder, ViewportCommand};

    fn builder(placement: &mut WindowPlacement, position: [f32; 2]) -> ViewportBuilder {
        let builder = ViewportBuilder::default();
        match placement.position(Some(position)) {
            Some(position) => builder.with_position(position),
            None => builder,
        }
    }

    #[test]
    fn dragging_does_not_issue_position_commands_back_to_windows() {
        let mut placement = WindowPlacement::default();
        let mut previous = builder(&mut placement, [100.0, 100.0]);
        for position in [[101.0, 100.0], [110.0, 105.0], [109.0, 105.0]] {
            let (commands, _) = previous.patch(builder(&mut placement, position));
            assert!(!commands
                .iter()
                .any(|command| matches!(command, ViewportCommand::OuterPosition(_))));
        }
        placement.reset();
        assert_eq!(
            placement.position(Some([110.0, 105.0])),
            Some([110.0, 105.0])
        );
    }

    #[test]
    fn a_window_with_no_saved_position_does_not_reposition_after_moving() {
        let mut placement = WindowPlacement::default();
        assert_eq!(placement.position(None), None);
        assert_eq!(placement.position(Some([500.0, 300.0])), None);
        placement.reset();
        assert_eq!(
            placement.position(Some([500.0, 300.0])),
            Some([500.0, 300.0])
        );
    }
}
