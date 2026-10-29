use super::interface_helpers::{age_label, chat_title};
use super::*;
use eframe::egui::{Rect, RichText, Stroke, StrokeKind, Vec2};

enum Choice {
    New,
    Saved(String),
}

impl App {
    pub(super) fn quick_chat_button(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.ui_language;
        let label = self
            .selected_chat
            .as_ref()
            .map(|chat| lang.format("Continue: {0}", chat_title(chat, lang)))
            .unwrap_or_else(|| lang.text("New conversation").to_owned());
        // Leave room for the three existing 36px toolbar controls and their gaps.
        let width = if self.settings.minimal {
            36.0
        } else {
            (ui.available_width() - 132.0).clamp(100.0, 190.0)
        };
        let visible_label = if self.settings.minimal { "" } else { &label };
        let response = ui
            .scope(|ui| {
                ui.spacing_mut().button_padding.x = 18.0;
                ui.add_sized(
                    [width, 36.0],
                    egui::Button::new(RichText::new(visible_label).size(12.0))
                        .truncate()
                        .corner_radius(10.0),
                )
            })
            .inner;
        let color = palette(self.settings.theme).muted;
        paint_picker_icon(ui, response.rect, self.settings.minimal, color);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!(
                    "{}: {}",
                    lang.text("Choose the conversation for your next session"),
                    label
                ),
            )
        });
        if response.clicked() {
            self.quick_chat_open = !self.quick_chat_open;
            if self.quick_chat_open {
                self.prepare_quick_chats();
            }
        }
        let response = response.on_hover_text(format!(
            "{}\n{}",
            lang.text("Choose the conversation for your next session"),
            label
        ));
        self.quick_chat_popup(ui, &response);
    }

    fn prepare_quick_chats(&mut self) {
        self.quick_chat_query.clear();
        self.refresh_library();
        if let Some(chat) = &self.selected_chat {
            if !self.library.iter().any(|saved| saved.id == chat.id) {
                self.library.insert(0, chat.clone());
                self.library_search.invalidate();
                self.quick_chat_search.invalidate();
            }
        }
    }

    fn quick_chat_popup(&mut self, ui: &egui::Ui, response: &egui::Response) {
        let mut open = self.quick_chat_open;
        let mut choice = None;
        egui::Popup::from_response(response)
            .open_bool(&mut open)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .gap(5.0)
            .width(260.0)
            .frame(
                egui::Frame::popup(ui.style())
                    .corner_radius(12.0)
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(|ui| self.quick_chat_menu(ui, &mut choice));
        if let Some(choice) = choice {
            if self.apply_quick_choice(choice) {
                open = false;
            }
        }
        self.quick_chat_open = open;
    }

    fn quick_chat_menu(&mut self, ui: &mut egui::Ui, choice: &mut Option<Choice>) {
        let lang = self.settings.ui_language;
        let colors = palette(self.settings.theme);
        ui.set_min_width(240.0);
        if ui
            .add_sized(
                [ui.available_width(), 30.0],
                egui::Button::selectable(
                    self.selected_chat.is_none(),
                    lang.text("New conversation"),
                )
                .truncate()
                .right_text(""),
            )
            .clicked()
        {
            *choice = Some(Choice::New);
        }
        ui.separator();
        ui.label(
            RichText::new(lang.text("Continue a conversation"))
                .size(12.0)
                .strong()
                .color(colors.ink),
        );
        if !self.library_warning.is_empty() {
            ui.label(RichText::new(lang.message(&self.library_warning)).color(colors.error));
        }
        if self.library.is_empty() {
            ui.label(RichText::new(lang.text("No saved conversations yet.")).color(colors.muted));
            return;
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.quick_chat_query)
                .hint_text(lang.text("Search saved conversations"))
                .desired_width(f32::INFINITY),
        );
        self.quick_chat_results(ui, choice);
    }

    fn quick_chat_results(&mut self, ui: &mut egui::Ui, choice: &mut Option<Choice>) {
        let lang = self.settings.ui_language;
        let matches = self
            .quick_chat_search
            .matching(&self.library, &self.quick_chat_query, lang);
        if matches.is_empty() {
            ui.label(
                RichText::new(lang.text("No conversations found."))
                    .color(palette(self.settings.theme).muted),
            );
            return;
        }
        let selected = self.selected_chat.as_ref().map(|chat| &chat.id);
        egui::ScrollArea::vertical()
            .id_salt("quick-chat-results")
            .max_height(114.0)
            .auto_shrink([false, true])
            .show_rows(ui, 30.0, matches.len(), |ui, rows| {
                for row in rows {
                    let chat = &self.library[matches[row]];
                    let title = chat_title(chat, lang);
                    let details = format!(
                        "{} · {} · {}",
                        title,
                        lang.text(&chat.language),
                        age_label(chat.updated_at, lang)
                    );
                    if ui
                        .add_sized(
                            [ui.available_width(), 30.0],
                            egui::Button::selectable(selected == Some(&chat.id), title)
                                .truncate()
                                .right_text(""),
                        )
                        .on_hover_text(details)
                        .clicked()
                    {
                        *choice = Some(Choice::Saved(chat.id.clone()));
                    }
                }
            });
    }

    fn apply_quick_choice(&mut self, choice: Choice) -> bool {
        match choice {
            Choice::New => {
                self.new_chat();
                self.selected_chat.is_none()
            }
            Choice::Saved(id) => self.choose_saved_chat(&id),
        }
    }

    fn choose_saved_chat(&mut self, id: &str) -> bool {
        if self
            .selected_chat
            .as_ref()
            .is_some_and(|chat| chat.id == id)
        {
            return true;
        }
        self.select_chat(id);
        if self
            .selected_chat
            .as_ref()
            .is_some_and(|chat| chat.id == id)
        {
            return true;
        }
        self.status = self.log_feedback.clone();
        self.error = true;
        false
    }
}

fn paint_picker_icon(ui: &egui::Ui, rect: Rect, minimal: bool, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let center = if minimal {
        let center = rect.center();
        ui.painter().rect_stroke(
            Rect::from_center_size(center + Vec2::new(-2.5, -1.0), Vec2::new(14.0, 11.0)),
            3.0,
            stroke,
            StrokeKind::Inside,
        );
        ui.painter().line_segment(
            [center + Vec2::new(-7.0, 4.5), center + Vec2::new(-7.0, 7.0)],
            stroke,
        );
        center + Vec2::new(7.0, 3.0)
    } else {
        rect.right_center() - Vec2::new(10.0, 0.0)
    };
    ui.painter().line_segment(
        [center + Vec2::new(-3.5, -2.0), center + Vec2::new(0.0, 1.5)],
        stroke,
    );
    ui.painter().line_segment(
        [center + Vec2::new(0.0, 1.5), center + Vec2::new(3.5, -2.0)],
        stroke,
    );
}

#[cfg(test)]
mod tests;
