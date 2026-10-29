use super::*;
use eframe::egui::{RichText, Vec2, ViewportBuilder, ViewportId};

impl App {
    pub(super) fn settings_panel(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut close = false;
        let mut save = false;
        let mut position = None;
        let mut builder = ViewportBuilder::default()
            .with_title(self.settings.ui_language.text("Speeching settings"))
            .with_inner_size([470.0, 590.0])
            .with_min_inner_size([360.0, 380.0]);
        if let Some(pos) = self
            .settings_placement
            .position(self.window_positions.settings)
        {
            builder = builder.with_position(pos);
        }
        ctx.show_viewport_immediate(ViewportId::from_hash_of("settings"), builder, |child, _| {
            close = child.input(|input| input.viewport().close_requested());
            self.capture_shortcut(child);
            position = child.input(|input| {
                input
                    .viewport()
                    .outer_rect
                    .map(|rect| [rect.left(), rect.top()])
            });
            self.settings_header(child);
            save = self.settings_actions(child);
            self.settings_content(child);
        });
        if let Some(pos) = position {
            if self.window_positions.settings != Some(pos) {
                self.window_positions.settings = Some(pos);
                self.positions_dirty = true;
            }
        }
        if save {
            close |= self.save_settings();
        }
        if close {
            self.close_settings();
        }
    }

    fn settings_header(&mut self, ctx: &mut egui::Ui) {
        let colors = palette(self.settings.theme);
        let lang = self.settings.ui_language;
        egui::Panel::top("settings-header")
            .frame(
                egui::Frame::new()
                    .fill(colors.paper)
                    .inner_margin(egui::Margin {
                        left: 20,
                        right: 20,
                        top: 18,
                        bottom: 10,
                    }),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(lang.text("Settings"))
                        .size(22.0)
                        .strong()
                        .color(colors.ink),
                );
                ui.add_space(8.0);
                self.settings_tabs(ui);
                if crate::languages::canonical(self.settings.provider, &self.settings.language)
                    .is_none()
                    && ui
                        .link(
                            RichText::new(lang.text("Choose a supported language"))
                                .color(colors.error),
                        )
                        .clicked()
                {
                    self.settings_section = SettingsSection::Practice;
                }
            });
    }

    fn settings_tabs(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.ui_language;
        let previous = self.settings_section;
        ui.horizontal_wrapped(|ui| {
            for (section, name) in [
                (SettingsSection::Practice, "Practice"),
                (SettingsSection::Microphone, "Microphone"),
                (SettingsSection::Provider, "Provider"),
                (SettingsSection::Appearance, "Appearance"),
            ] {
                ui.selectable_value(&mut self.settings_section, section, lang.text(name));
            }
        });
        if self.settings_section == previous {
            return;
        }
        if previous == SettingsSection::Provider && !self.persist_pending_keys() {
            self.settings_section = SettingsSection::Provider;
        }
        if self.settings_section != SettingsSection::Microphone {
            self.shortcut_capture = false;
        }
        if self.key_feedback == "Saved API key removed." {
            self.key_feedback.clear();
        }
    }

    fn settings_actions(&mut self, ctx: &mut egui::Ui) -> bool {
        let colors = palette(self.settings.theme);
        let lang = self.settings.ui_language;
        egui::Panel::bottom("settings-actions")
            .frame(
                egui::Frame::new()
                    .fill(colors.paper)
                    .inner_margin(egui::Margin {
                        left: 20,
                        right: 20,
                        top: 10,
                        bottom: 16,
                    }),
            )
            .show(ctx, |ui| {
                if !self.key_feedback.is_empty() {
                    let color = if self.key_feedback == "Saved API key removed." {
                        colors.muted
                    } else {
                        colors.error
                    };
                    ui.label(
                        RichText::new(lang.message(&self.key_feedback))
                            .size(12.0)
                            .color(color),
                    );
                }
                let valid =
                    crate::languages::canonical(self.settings.provider, &self.settings.language)
                        .is_some();
                ui.add_enabled(
                    valid,
                    egui::Button::new(
                        RichText::new(lang.text("Save settings"))
                            .color(Color32::WHITE)
                            .strong(),
                    )
                    .fill(BLUE)
                    .corner_radius(7.0)
                    .min_size(Vec2::new(130.0, 36.0)),
                )
                .clicked()
            })
            .inner
    }

    fn settings_content(&mut self, ctx: &mut egui::Ui) {
        let colors = palette(self.settings.theme);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(colors.paper)
                    .inner_margin(egui::Margin {
                        left: 20,
                        right: 20,
                        top: 12,
                        bottom: 8,
                    }),
            )
            .show(ctx, |ui| {
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt(("settings-content", self.settings_section as u8))
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.settings_section {
                        SettingsSection::Practice => self.practice_settings(ui),
                        SettingsSection::Microphone => self.microphone_settings(ui),
                        SettingsSection::Provider => self.provider_settings(ui),
                        SettingsSection::Appearance => self.appearance_settings(ui),
                    });
            });
    }

    fn save_settings(&mut self) -> bool {
        if let Some(language) =
            crate::languages::canonical(self.settings.provider, &self.settings.language)
        {
            self.settings.language = language.to_owned();
        }
        match self.save_pending_keys().and_then(|_| self.settings.save()) {
            Ok(()) => {
                if self.session.is_none() {
                    self.status = "Ready when you are".into();
                    self.error = false;
                }
                true
            }
            Err(error) => {
                self.key_feedback = error.clone();
                self.status = format!("Could not save settings: {error}");
                self.error = true;
                false
            }
        }
    }
}
