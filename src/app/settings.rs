use super::interface::{is_modifier_key, pace_label, voice_description, voice_symbol};
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
        let colors = palette(self.settings.theme);
        let lang = self.settings.ui_language;
        let mut builder = ViewportBuilder::default()
            .with_title(lang.text("Speeching settings"))
            .with_inner_size([470.0, 590.0])
            .with_min_inner_size([360.0, 380.0]);
        if let Some(pos) = self.window_positions.settings {
            builder = builder.with_position(pos);
        }
        ctx.show_viewport_immediate(ViewportId::from_hash_of("settings"), builder, |child, _| {
            if child.input(|i| i.viewport().close_requested()) {
                close = true;
            }
            self.capture_shortcut(child);
            position = child.input(|i| i.viewport().outer_rect.map(|r| [r.left(), r.top()]));
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
                .show(child, |ui| {
                    ui.label(
                        RichText::new(lang.text("Settings"))
                            .size(22.0)
                            .strong()
                            .color(colors.ink),
                    );
                    ui.add_space(8.0);
                    let previous_section = self.settings_section;
                    ui.horizontal_wrapped(|ui| {
                        for (section, name) in [
                            (SettingsSection::Practice, "Practice"),
                            (SettingsSection::Microphone, "Microphone"),
                            (SettingsSection::Provider, "Provider"),
                            (SettingsSection::Appearance, "Appearance"),
                        ] {
                            ui.selectable_value(
                                &mut self.settings_section,
                                section,
                                lang.text(name),
                            );
                        }
                    });
                    if self.settings_section != previous_section {
                        if previous_section == SettingsSection::Provider
                            && !self.persist_pending_keys()
                        {
                            self.settings_section = SettingsSection::Provider;
                        }
                        if self.settings_section != SettingsSection::Microphone {
                            self.shortcut_capture = false;
                        }
                        if self.key_feedback == "Saved API key removed." {
                            self.key_feedback.clear();
                        }
                    }
                    if crate::languages::canonical(self.settings.provider, &self.settings.language)
                        .is_none()
                    {
                        if ui
                            .link(
                                RichText::new(lang.text("Choose a supported language"))
                                    .color(colors.error),
                            )
                            .clicked()
                        {
                            self.settings_section = SettingsSection::Practice;
                        }
                    }
                });
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
                .show(child, |ui| {
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
                    let valid = crate::languages::canonical(
                        self.settings.provider,
                        &self.settings.language,
                    )
                    .is_some();
                    if ui
                        .add_enabled(
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
                    {
                        save = true;
                    }
                });
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
                .show(child, |ui| {
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
        });
        if let Some(pos) = position {
            if self.window_positions.settings != Some(pos) {
                self.window_positions.settings = Some(pos);
                self.positions_dirty = true;
            }
        }
        if save {
            if let Some(language) =
                crate::languages::canonical(self.settings.provider, &self.settings.language)
            {
                self.settings.language = language.to_owned();
            }
            match self.save_pending_keys().and_then(|_| self.settings.save()) {
                Ok(()) => {
                    close = true;
                    if self.session.is_none() {
                        self.status = "Ready when you are".into();
                        self.error = false;
                    }
                }
                Err(error) => {
                    self.key_feedback = error.clone();
                    self.status = format!("Could not save settings: {error}");
                    self.error = true;
                }
            }
        }
        if close {
            self.close_settings();
        }
    }

    fn capture_shortcut(&mut self, ctx: &egui::Context) {
        if !self.shortcut_capture {
            return;
        }
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } if !is_modifier_key(*key) => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((key, modifiers)) = pressed {
            if key != egui::Key::Escape {
                self.settings.push_to_talk_shortcut = Shortcut {
                    key: key.name().into(),
                    ctrl: modifiers.ctrl,
                    alt: modifiers.alt,
                    shift: modifiers.shift,
                };
            }
            self.shortcut_capture = false;
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { .. })));
        }
    }

    fn practice_settings(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.ui_language;
        let colors = palette(self.settings.theme);
        self.language_selector(ui);
        ui.add_space(8.0);
        practice_pickers(ui, &mut self.settings, lang);
        ui.label(RichText::new(lang.text("Adjusts the partner's speech while preserving voice pitch. Applies to the next conversation.")).size(12.0).color(colors.muted));
        ui.add_space(12.0);
        ui.label(RichText::new(lang.text("Voice")).strong());
        let voice = match self.settings.provider {
            Provider::Gemini => &mut self.settings.gemini_voice,
            Provider::OpenAi => &mut self.settings.openai_voice,
        };
        egui::ComboBox::from_id_salt("voice")
            .selected_text(format!(
                "{}  {}",
                voice_symbol(self.settings.provider, voice),
                voice
            ))
            .width(230.0)
            .show_ui(ui, |ui| match self.settings.provider {
                Provider::Gemini => {
                    for (name, description) in GEMINI_VOICES {
                        ui.selectable_value(
                            voice,
                            (*name).to_owned(),
                            format!(
                                "{}  {name} · {}",
                                voice_symbol(Provider::Gemini, name),
                                voice_description(lang, description)
                            ),
                        );
                    }
                }
                Provider::OpenAi => {
                    for name in OPENAI_VOICES {
                        ui.selectable_value(
                            voice,
                            (*name).to_owned(),
                            format!("{}  {name}", voice_symbol(Provider::OpenAi, name)),
                        );
                    }
                }
            });
        ui.label(
            RichText::new(lang.text(
                "Voice symbols describe perceived register; individual perception can vary.",
            ))
            .size(12.0)
            .color(colors.muted),
        );
        ui.add_space(12.0);
        ui.label(RichText::new(lang.text("Your instructions")).strong());
        ui.add(
            egui::TextEdit::multiline(&mut self.settings.instructions)
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .hint_text(
                    lang.text("e.g. Correct my mistakes gently and ask about everyday life"),
                ),
        );
        ui.label(
            RichText::new(
                lang.text("Added to the partner's system instructions for the next conversation."),
            )
            .size(12.0)
            .color(colors.muted),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(lang.text(
                "Provider, language, voice and pace changes apply to the next conversation.",
            ))
            .size(12.0)
            .color(colors.muted),
        );
    }

    fn microphone_settings(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.ui_language;
        let colors = palette(self.settings.theme);
        ui.label(RichText::new(lang.text("Microphone")).size(16.0).strong());
        ui.add_space(5.0);
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.settings.input_mode,
                InputMode::VoiceActivated,
                lang.text("Voice activated"),
            );
            ui.selectable_value(
                &mut self.settings.input_mode,
                InputMode::PushToTalk,
                lang.text("Push to talk"),
            );
        });
        if self.settings.input_mode == InputMode::PushToTalk {
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(lang.text("Shortcut"));
                let label = if self.shortcut_capture {
                    lang.text("Press a key combination…").to_owned()
                } else {
                    self.settings.push_to_talk_shortcut.label_in(lang)
                };
                if ui.button(label).clicked() {
                    self.shortcut_capture = true;
                }
            });
            ui.label(
                RichText::new(lang.text(if self.shortcut_capture {
                    "Press a key with optional Ctrl, Alt or Shift. Esc cancels."
                } else {
                    "Hold this shortcut while the main window is focused; release to send."
                }))
                .size(12.0)
                .color(colors.muted),
            );
        } else {
            self.shortcut_capture = false;
            ui.label(
                RichText::new(lang.text("Speech is detected automatically."))
                    .size(12.0)
                    .color(colors.muted),
            );
        }
        ui.add_space(16.0);
        ui.checkbox(
            &mut self.settings.noise_gate_enabled,
            lang.text("Reduce quiet background noise"),
        );
        ui.add_enabled_ui(self.settings.noise_gate_enabled, |ui| {
            ui.add(
                egui::Slider::new(&mut self.settings.noise_gate_db, -65.0..=-20.0)
                    .text(lang.text("Noise threshold"))
                    .suffix(" dB")
                    .step_by(1.0),
            );
        });
        ui.label(
            RichText::new(lang.text(
                "Raise the threshold for a noisier room; too high can suppress quiet speech.",
            ))
            .size(12.0)
            .color(colors.muted),
        );
    }

    fn provider_settings(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.ui_language;
        let colors = palette(self.settings.theme);
        ui.label(RichText::new(lang.text("Provider")).size(16.0).strong());
        let previous_provider = self.settings.provider;
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.settings.provider, Provider::Gemini, "Gemini Live");
            ui.selectable_value(
                &mut self.settings.provider,
                Provider::OpenAi,
                "OpenAI Realtime",
            );
        });
        if self.settings.provider != previous_provider && !self.persist_pending_keys() {
            self.settings.provider = previous_provider;
        }
        ui.add_space(12.0);
        let provider = self.settings.provider;
        let saved = match provider {
            Provider::Gemini => self.gemini_key_saved,
            Provider::OpenAi => self.openai_key_saved,
        };
        ui.label(RichText::new(lang.text("API key")).strong());
        if saved && self.editing_key != Some(provider) {
            ui.label(
                RichText::new(lang.text("Saved securely in Windows Credential Manager."))
                    .size(12.0)
                    .color(colors.muted),
            );
            ui.horizontal_wrapped(|ui| {
                if ui.small_button(lang.text("Replace key")).clicked() {
                    self.editing_key = Some(provider);
                }
                if ui.small_button(lang.text("Remove saved key")).clicked() {
                    self.forget_key(provider);
                }
            });
        } else {
            let draft = match provider {
                Provider::Gemini => &mut self.gemini_key,
                Provider::OpenAi => &mut self.openai_key,
            };
            let response = ui.add(
                egui::TextEdit::singleline(draft)
                    .password(true)
                    .hint_text(lang.text("Paste an API key"))
                    .desired_width(f32::INFINITY),
            );
            let has_draft = !draft.trim().is_empty();
            let pasted = response.changed()
                && ui.input(|input| {
                    input
                        .events
                        .iter()
                        .any(|event| matches!(event, egui::Event::Paste(_)))
                });
            ui.label(
                RichText::new(lang.text("Saved automatically when you paste or leave this field."))
                    .size(12.0)
                    .color(colors.muted),
            );
            let mut save_key = false;
            let mut cancel = false;
            ui.horizontal_wrapped(|ui| {
                save_key = ui
                    .add_enabled(has_draft, egui::Button::new(lang.text("Save key")))
                    .clicked();
                if saved {
                    cancel = ui.small_button(lang.text("Cancel")).clicked();
                }
            });
            if cancel {
                draft.clear();
                self.editing_key = None;
                self.key_feedback.clear();
            } else if has_draft && (pasted || response.lost_focus() || save_key) {
                self.persist_pending_keys();
            }
        }
        ui.add_space(12.0);
        let (url, model) = match provider {
            Provider::Gemini => (
                &mut self.settings.gemini_url,
                &mut self.settings.gemini_model,
            ),
            Provider::OpenAi => (
                &mut self.settings.openai_url,
                &mut self.settings.openai_model,
            ),
        };
        ui.label(RichText::new(lang.text("Model")).strong());
        ui.add(egui::TextEdit::singleline(model).desired_width(f32::INFINITY));
        ui.add_space(8.0);
        ui.label(RichText::new(lang.text("WebSocket URL")).strong());
        ui.add(egui::TextEdit::singleline(url).desired_width(f32::INFINITY));
        ui.label(RichText::new(lang.text("The selected URL receives this key. Custom endpoints must use the provider's live protocol.")).size(12.0).color(colors.muted));
    }

    fn appearance_settings(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.ui_language;
        ui.label(RichText::new(lang.text("Interface language")).strong());
        egui::ComboBox::from_id_salt("interface-language")
            .selected_text(self.settings.ui_language.native_name())
            .show_ui(ui, |ui| {
                for option in UiLanguage::ALL {
                    ui.selectable_value(
                        &mut self.settings.ui_language,
                        option,
                        option.native_name(),
                    );
                }
            });
        if lang != self.settings.ui_language {
            if let Some(name) =
                crate::languages::canonical(self.settings.provider, &self.settings.language)
            {
                self.language_input = self.settings.ui_language.text(name).to_owned();
            }
        }
        ui.add_space(16.0);
        ui.label(RichText::new(lang.text("Appearance")).strong());
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.settings.theme, Theme::Light, lang.text("Light"));
            ui.selectable_value(&mut self.settings.theme, Theme::Dark, lang.text("Dark"));
        });
        ui.add_space(16.0);
        ui.checkbox(
            &mut self.settings.minimal,
            lang.text("Hide conversation text"),
        );
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::Slider::new(&mut self.settings.fade_seconds, 2.0..=30.0)
                    .text(lang.text("Bubble lifetime"))
                    .suffix(" s")
                    .step_by(1.0),
            );
            if ui.small_button(lang.text("Reset to 8 s")).clicked() {
                self.settings.fade_seconds = 8.0;
            }
        });
    }
}

fn practice_pickers(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    lang: UiLanguage,
) -> [egui::Rect; 2] {
    let level = ui
        .horizontal_wrapped(|ui| {
            ui.label(lang.text("Level"));
            egui::ComboBox::from_id_salt("level")
                .selected_text(lang.text(&settings.level))
                .show_ui(ui, |ui| {
                    for value in ["Beginner", "Intermediate", "Advanced"] {
                        ui.selectable_value(&mut settings.level, value.into(), lang.text(value));
                    }
                })
                .response
                .rect
        })
        .inner;
    let pace = ui
        .horizontal_wrapped(|ui| {
            ui.label(lang.text("Speaking pace"));
            egui::ComboBox::from_id_salt("pace")
                .selected_text(lang.text(pace_label(settings.speed)))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (0.75, "Very slow"),
                        (0.82, "A little slower"),
                        (1.0, "Natural"),
                        (1.15, "A little faster"),
                        (1.3, "Fast"),
                    ] {
                        ui.selectable_value(&mut settings.speed, value, lang.text(label));
                    }
                })
                .response
                .rect
        })
        .inner;
    [level, pace]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn localized_practice_pickers_fit_the_narrow_settings_body() {
        for language in UiLanguage::ALL {
            let ctx = egui::Context::default();
            let mut settings = Settings::default();
            let mut positions = [egui::Rect::NOTHING; 2];
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(280.0, 400.0),
                )),
                ..Default::default()
            };
            let mut output = ctx.run_ui(raw, |ui| {
                positions = practice_pickers(ui, &mut settings, language);
            });
            output.textures_delta.clear();
            for rect in positions {
                assert!(
                    rect.right() <= 280.0,
                    "{language:?}: picker extends to {}",
                    rect.right()
                );
            }
        }
    }
}
