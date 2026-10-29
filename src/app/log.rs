use super::interface_helpers::chat_title;
use super::*;
use eframe::egui::{RichText, ViewportBuilder, ViewportId};

impl App {
    pub(super) fn log_panel(&mut self, ctx: &egui::Context) {
        if !self.log_open {
            return;
        }
        let mut close = false;
        let mut position = None;
        let mut selected_id = None;
        let lang = self.settings.ui_language;
        let mut builder = ViewportBuilder::default()
            .with_title(lang.text("Speeching conversation log"))
            .with_inner_size([440.0, 500.0])
            .with_min_inner_size([320.0, 300.0]);
        if let Some(pos) = self.log_placement.position(self.window_positions.log) {
            builder = builder.with_position(pos);
        }
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("conversation-log"),
            builder,
            |child, _| {
                close = child.input(|input| input.viewport().close_requested());
                position = child.input(|input| {
                    input
                        .viewport()
                        .outer_rect
                        .map(|rect| [rect.left(), rect.top()])
                });
                self.log_window(child, &mut selected_id);
            },
        );
        if let Some(id) = selected_id {
            self.select_chat(&id);
        }
        if let Some(pos) = position {
            if self.window_positions.log != Some(pos) {
                self.window_positions.log = Some(pos);
                self.positions_dirty = true;
            }
        }
        if close {
            self.log_open = false;
            self.log_placement.reset();
            self.delete_confirm = false;
            self.rename_active = false;
            self.save_positions();
        }
    }

    fn log_window(&mut self, ctx: &mut egui::Ui, selected_id: &mut Option<String>) {
        let colors = palette(self.settings.theme);
        egui::Panel::top("log-header")
            .frame(
                egui::Frame::new()
                    .fill(colors.paper)
                    .inner_margin(egui::Margin {
                        left: 18,
                        right: 18,
                        top: 16,
                        bottom: 8,
                    }),
            )
            .show(ctx, |ui| self.log_header(ui));
        if !self.library_open {
            egui::Panel::bottom("log-actions")
                .frame(
                    egui::Frame::new()
                        .fill(colors.paper)
                        .inner_margin(egui::Margin {
                            left: 18,
                            right: 18,
                            top: 8,
                            bottom: 14,
                        }),
                )
                .show(ctx, |ui| {
                    if !self.log_feedback.is_empty() {
                        ui.label(
                            RichText::new(self.settings.ui_language.message(&self.log_feedback))
                                .size(12.0)
                                .color(colors.muted),
                        );
                    }
                    self.log_actions(ui);
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(colors.paper)
                    .inner_margin(egui::Margin {
                        left: 18,
                        right: 18,
                        top: 6,
                        bottom: 4,
                    }),
            )
            .show(ctx, |ui| {
                ui.add_space(4.0);
                if self.library_open {
                    self.log_library(ui, selected_id);
                } else {
                    self.log_transcript(ui);
                }
            });
        self.delete_confirmation(ctx);
    }

    fn log_header(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.ui_language;
        let colors = palette(self.settings.theme);
        let heading = if self.library_open {
            lang.text("Saved conversations")
        } else if let Some(chat) = &self.selected_chat {
            chat_title(chat, lang)
        } else {
            lang.text("Conversation log")
        };
        ui.add(
            egui::Label::new(RichText::new(heading).size(21.0).strong().color(colors.ink))
                .truncate(),
        )
        .on_hover_text(heading);
        ui.add_space(6.0);
        self.log_navigation(ui);
        if !self.library_open {
            self.log_chat_controls(ui);
        }
    }

    fn log_navigation(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.ui_language;
        ui.horizontal_wrapped(|ui| {
            if self.library_open {
                if ui.button(lang.text("Back to conversation")).clicked() {
                    self.library_open = false;
                }
            } else if ui
                .add_enabled(
                    self.session.is_none(),
                    egui::Button::new(lang.text("Saved chats…")),
                )
                .clicked()
            {
                self.refresh_library();
                self.library_open = true;
                self.rename_active = false;
            }
            if ui
                .add_enabled(
                    self.session.is_none(),
                    egui::Button::new(lang.text("New conversation")),
                )
                .clicked()
            {
                self.new_chat();
            }
        });
    }

    fn log_chat_controls(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.ui_language;
        let Some(title) = self
            .selected_chat
            .as_ref()
            .map(|chat| chat_title(chat, lang).to_owned())
        else {
            ui.add_space(8.0);
            ui.label(
                RichText::new(lang.text("New conversation"))
                    .color(palette(self.settings.theme).muted),
            );
            return;
        };
        if self.rename_active {
            self.log_rename_controls(ui);
        } else {
            self.log_saved_chat_controls(ui, title);
        }
    }

    fn log_rename_controls(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.ui_language;
        ui.horizontal_wrapped(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.rename_draft)
                    .char_limit(100)
                    .desired_width(180.0)
                    .hint_text(lang.text("Conversation name")),
            );
            let has_name = !self.rename_draft.trim().is_empty();
            if ui
                .add_enabled(has_name, egui::Button::new(lang.text("Save")))
                .clicked()
                || (response.lost_focus()
                    && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    && has_name)
            {
                self.rename_chat();
            }
            if ui.button(lang.text("Cancel")).clicked() {
                self.rename_active = false;
            }
        });
    }

    fn log_saved_chat_controls(&mut self, ui: &mut egui::Ui, title: String) {
        let lang = self.settings.ui_language;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.session.is_none(),
                    egui::Button::new(lang.text("Rename")),
                )
                .clicked()
            {
                self.rename_draft = title;
                self.rename_active = true;
                self.log_feedback.clear();
            }
            if ui
                .add_enabled(
                    self.session.is_none(),
                    egui::Button::new(lang.text("Delete conversation")),
                )
                .clicked()
            {
                self.delete_confirm = true;
            }
        });
    }

    fn log_library(&mut self, ui: &mut egui::Ui, selected_id: &mut Option<String>) {
        let lang = self.settings.ui_language;
        let colors = palette(self.settings.theme);
        ui.add(
            egui::TextEdit::singleline(&mut self.library_query)
                .hint_text(lang.text("Search saved conversations"))
                .desired_width(f32::INFINITY),
        );
        self.conversation_sort_control(ui, "log-chat-sort");
        ui.add_space(8.0);
        if !self.library_warning.is_empty() {
            ui.label(
                RichText::new(lang.message(&self.library_warning))
                    .size(12.0)
                    .color(colors.error),
            );
        }
        let matching = self.library_search.matching(
            &self.library,
            &self.library_query,
            lang,
            self.conversation_view.order,
        );
        let library = &self.library;
        let selected_chat = self.selected_chat.as_ref();
        egui::ScrollArea::vertical()
            .id_salt("saved-chats")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if matching.is_empty() {
                    ui.label(
                        RichText::new(lang.text("No conversations found.")).color(colors.muted),
                    );
                }
                for index in matching {
                    let chat = &library[*index];
                    let selected = selected_chat.is_some_and(|current| current.id == chat.id);
                    if ui
                        .selectable_label(selected, chat_title(chat, lang))
                        .clicked()
                    {
                        *selected_id = Some(chat.id.clone());
                    }
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            lang.text(&chat.language),
                            lang.format(
                                "Last updated: {0}",
                                &date_time::full(chat.updated_at, lang)
                            )
                        ))
                        .size(11.0)
                        .color(colors.muted),
                    );
                    ui.add_space(8.0);
                }
            });
    }

    fn log_transcript(&self, ui: &mut egui::Ui) {
        let lang = self.settings.ui_language;
        let colors = palette(self.settings.theme);
        let accent = if self.settings.theme == Theme::Dark {
            Color32::from_rgb(145, 186, 255)
        } else {
            BLUE
        };
        egui::ScrollArea::vertical()
            .id_salt("transcript")
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.transcript.sessions.is_empty() {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(lang.text("Your conversation log will appear here."))
                            .color(colors.muted),
                    );
                }
                if self.selected_chat.is_some() && self.session.is_none() {
                    ui.label(
                        RichText::new(lang.text(if self.history_truncated {
                            "Recent messages will be sent; older messages remain in the log."
                        } else {
                            "Previous messages will be sent to your provider when you start."
                        }))
                        .size(11.0)
                        .color(colors.muted),
                    );
                }
                for (index, session) in self.transcript.sessions.iter().enumerate() {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            lang.text("Conversation"),
                            format_duration(session.clock.elapsed(Instant::now()))
                        ))
                        .size(12.0)
                        .color(colors.muted),
                    );
                    for line in self
                        .transcript
                        .lines
                        .iter()
                        .filter(|line| line.session == index)
                    {
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(self.transcript.speaker_name(line, lang))
                                .strong()
                                .color(accent),
                        );
                        ui.label(&line.text);
                    }
                }
            });
    }

    fn log_actions(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.ui_language;
        let has_text = !self.transcript.sessions.is_empty();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(has_text, egui::Button::new(lang.text("Copy")))
                .clicked()
            {
                ui.ctx().copy_text(self.transcript.text(lang));
                self.status = "Conversation copied".into();
                self.log_feedback = "Copied to clipboard".into();
            }
            if ui
                .add_enabled(has_text, egui::Button::new(lang.text("Download")))
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("speeching-conversation.txt")
                    .add_filter(lang.text("Text file"), &["txt"])
                    .save_file()
                {
                    match std::fs::write(path, self.transcript.text(lang)) {
                        Ok(()) => {
                            self.status = "Conversation saved".into();
                            self.log_feedback = "Conversation saved".into();
                        }
                        Err(_) => {
                            self.status = "Could not save the conversation".into();
                            self.log_feedback = self.status.clone();
                            self.error = true;
                        }
                    }
                }
            }
            if ui
                .add_enabled(has_text, egui::Button::new(lang.text("Clear")))
                .clicked()
            {
                self.transcript.clear();
                self.history_dirty = self.selected_chat.is_some();
                if self.flush_history() {
                    self.status = "Conversation log cleared".into();
                    self.log_feedback = "Conversation cleared".into();
                }
            }
        });
    }

    fn delete_confirmation(&mut self, ctx: &egui::Context) {
        if !self.delete_confirm {
            return;
        }
        let Some(chat) = &self.selected_chat else {
            self.delete_confirm = false;
            return;
        };
        let lang = self.settings.ui_language;
        let title = chat_title(chat, lang).to_owned();
        let mut confirmed = false;
        let mut canceled = false;
        let modal = egui::Modal::new(egui::Id::new("delete-conversation-confirmation")).show(ctx, |ui| {
            ui.set_max_width(300.0);
            ui.label(RichText::new(lang.text("Delete conversation?")).size(18.0).strong());
            ui.add_space(6.0);
            ui.label(lang.format("Delete “{0}”? This permanently removes the saved conversation from this computer.", &title));
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button(lang.text("Cancel")).clicked() { canceled = true; }
                if ui.add(egui::Button::new(RichText::new(lang.text("Delete conversation")).color(Color32::WHITE)).fill(Color32::from_rgb(157, 55, 59))).clicked() { confirmed = true; }
            });
        });
        if confirmed {
            self.delete_chat();
        } else if canceled || modal.should_close() {
            self.delete_confirm = false;
        }
    }
}
