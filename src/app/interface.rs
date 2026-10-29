use super::*;
use eframe::egui::{
    Align, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2, ViewportBuilder,
    ViewportId,
};
use std::time::{Duration, Instant};

impl App {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let colors = palette(self.settings.theme);
        ui.horizontal(|ui| {
            if !self.settings.minimal {
                ui.label(
                    RichText::new("speeching")
                        .strong()
                        .size(16.0)
                        .color(colors.ink),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, Icon::Settings, self.settings_open)
                    .on_hover_text(if self.settings_open {
                        "Close settings"
                    } else {
                        "Open settings"
                    })
                    .clicked()
                {
                    self.settings_open = !self.settings_open;
                    if !self.settings_open {
                        self.save_positions();
                    }
                }
                if icon_button(ui, Icon::Log, self.log_open)
                    .on_hover_text(if self.log_open {
                        "Close conversation log"
                    } else {
                        "Open conversation log"
                    })
                    .clicked()
                {
                    self.log_open = !self.log_open;
                    if !self.log_open {
                        self.save_positions();
                    }
                }
                if icon_button(ui, Icon::Eye, self.settings.minimal)
                    .on_hover_text(if self.settings.minimal {
                        "Show bubbles"
                    } else {
                        "Hide conversation text"
                    })
                    .clicked()
                {
                    self.settings.minimal = !self.settings.minimal;
                    if let Err(error) = self.settings.save() {
                        self.status = format!("Could not save settings: {error}");
                        self.error = true;
                    }
                }
                if self.connected
                    && icon_button(ui, Icon::Mic, self.muted)
                        .on_hover_text(if self.muted {
                            "Unmute microphone"
                        } else {
                            "Mute microphone"
                        })
                        .clicked()
                {
                    self.muted = !self.muted;
                    audio::play_cue(if self.muted {
                        Cue::MicrophoneOff
                    } else {
                        Cue::MicrophoneOn
                    });
                    if let Some(session) = &self.session {
                        session.mute(self.muted);
                    }
                }
            });
        });
    }

    fn microphone(&mut self, ui: &mut egui::Ui, size: f32) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, size), Sense::hover());
        let radius = size.min(155.0) * 0.40;
        let hit_area = Rect::from_center_size(rect.center(), Vec2::splat(radius * 2.0 + 20.0));
        let response = ui.interact(hit_area, ui.id().with("main-microphone"), Sense::click());
        let active = self.session.is_some();
        let label = if active {
            "End conversation"
        } else {
            "Start conversation"
        };
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, !self.stopping, label)
        });
        let center = rect.center();
        let p = ui.painter_at(rect);
        let (input, output) = self.levels();
        let pulse = (input * 6.0).max(output * 5.0).clamp(0.0, 1.0);
        let idle_time = self.idle_since.elapsed().as_secs_f32() % 13.0;
        let nudge = if !active && (0.7..2.1).contains(&idle_time) {
            let t = idle_time - 0.7;
            (t * 17.0).sin() * (1.0 - t / 1.4) * 3.0
        } else {
            0.0
        };
        let ring_center = center + Vec2::new(nudge, 0.0);
        p.circle_stroke(
            ring_center,
            radius + 9.0 + pulse * 3.0,
            Stroke::new(
                1.0_f32,
                if active {
                    Color32::from_rgb(222, 183, 184)
                } else {
                    Color32::from_rgb(196, 214, 239)
                },
            ),
        );
        p.circle_stroke(
            center - Vec2::new(nudge * 0.5, 0.0),
            radius + 5.0,
            Stroke::new(
                1.0_f32,
                if active {
                    Color32::from_rgb(196, 131, 136)
                } else {
                    Color32::from_rgb(149, 180, 224)
                },
            ),
        );
        p.circle_filled(
            center,
            radius,
            if active && response.hovered() {
                Color32::from_rgb(119, 39, 48)
            } else if active {
                Color32::from_rgb(147, 48, 58)
            } else if response.hovered() {
                Color32::from_rgb(29, 83, 198)
            } else {
                BLUE
            },
        );
        let white = Color32::WHITE;
        let line = Stroke::new(2.6_f32, white);
        p.rect_filled(
            Rect::from_center_size(center + Vec2::new(0.0, -8.0), Vec2::new(15.0, 29.0)),
            6.0,
            white,
        );
        p.line_segment(
            [
                center + Vec2::new(-12.0, -5.0),
                center + Vec2::new(-12.0, 10.0),
            ],
            line,
        );
        p.line_segment(
            [
                center + Vec2::new(-12.0, 10.0),
                center + Vec2::new(0.0, 16.0),
            ],
            line,
        );
        p.line_segment(
            [
                center + Vec2::new(0.0, 16.0),
                center + Vec2::new(12.0, 10.0),
            ],
            line,
        );
        p.line_segment(
            [
                center + Vec2::new(12.0, 10.0),
                center + Vec2::new(12.0, -5.0),
            ],
            line,
        );
        p.line_segment(
            [center + Vec2::new(0.0, 16.0), center + Vec2::new(0.0, 23.0)],
            line,
        );
        if response.clicked() && !self.stopping {
            if active {
                self.stop();
            } else {
                self.start();
            }
        }
        response.on_hover_text(label);
    }

    fn waveform(&self, ui: &mut egui::Ui, height: f32) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
        let painter = ui.painter_at(rect);
        let (input, output) = self.levels();
        let activity = (input * 6.5).max(output * 5.0).clamp(0.0, 1.0);
        let time = ui.ctx().input(|i| i.time) as f32;
        let colors = [
            Color32::from_rgba_unmultiplied(35, 99, 225, 220),
            Color32::from_rgba_unmultiplied(76, 139, 242, 150),
            Color32::from_rgba_unmultiplied(128, 181, 249, 165),
        ];
        for (layer, color) in colors.into_iter().enumerate() {
            let mut points = Vec::with_capacity(73);
            for i in 0..=72 {
                let x = i as f32 / 72.0;
                let envelope = wave_envelope(x);
                let motion = if activity > 0.015 {
                    time * (2.3 + layer as f32 * 0.4)
                } else {
                    0.0
                };
                let oscillation =
                    (x * (16.0 + layer as f32 * 2.8) + motion + layer as f32 * 1.3).sin();
                let amplitude =
                    (2.5 + activity * (height * 0.43)) * envelope * (1.0 - layer as f32 * 0.12);
                points.push(Pos2::new(
                    rect.left() + x * rect.width(),
                    rect.center().y + oscillation * amplitude,
                ));
            }
            painter.add(egui::Shape::line(
                points,
                Stroke::new(if layer == 0 { 2.0_f32 } else { 1.35_f32 }, color),
            ));
        }
    }

    fn language_selector(&mut self, ui: &mut egui::Ui) {
        let colors = palette(self.settings.theme);
        let valid =
            crate::languages::canonical(self.settings.provider, &self.settings.language).is_some();
        ui.horizontal(|ui| {
            ui.label("Language");
            if !valid {
                ui.label(
                    RichText::new("Choose a supported language")
                        .size(11.0)
                        .color(colors.error),
                );
            }
        });
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.settings.language)
                .desired_width(f32::INFINITY)
                .hint_text("Search supported languages"),
        );
        if response.gained_focus() || response.changed() {
            self.language_suggestions_open = true;
        }
        let matching = crate::languages::matching(self.settings.provider, &self.settings.language);
        let mut open = self.language_suggestions_open;
        let mut selected = None;
        egui::Popup::from_response(&response)
            .open_bool(&mut open)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .gap(4.0)
            .width(response.rect.width())
            .frame(
                egui::Frame::popup(ui.style())
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(|ui| {
                ui.set_min_width((response.rect.width() - 16.0).max(100.0));
                if matching.is_empty() {
                    ui.label(
                        RichText::new("No matches. Try another language.")
                            .size(12.0)
                            .color(colors.muted),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for language in matching {
                                if ui.selectable_label(false, language).clicked() {
                                    selected = Some(language);
                                }
                            }
                        });
                }
            });
        if let Some(language) = selected {
            self.settings.language = language.to_owned();
            open = false;
        }
        self.language_suggestions_open = open;
    }

    fn settings_panel(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut close = false;
        let mut save = false;
        let mut position = None;
        let colors = palette(self.settings.theme);
        let mut builder = ViewportBuilder::default()
            .with_title("Speeching settings")
            .with_inner_size([460.0, 690.0])
            .with_min_inner_size([360.0, 400.0]);
        if let Some(pos) = self.window_positions.settings {
            builder = builder.with_position(pos);
        }
        ctx.show_viewport_immediate(ViewportId::from_hash_of("settings"), builder, |child, _| {
            if child.input(|i| i.viewport().close_requested()) { close = true; }
            if self.shortcut_capture {
                let pressed = child.input(|i| i.events.iter().find_map(|event| match event {
                    egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } if !is_modifier_key(*key) => Some((*key, *modifiers)),
                    _ => None,
                }));
                if let Some((key, modifiers)) = pressed {
                    if key != egui::Key::Escape {
                        self.settings.push_to_talk_shortcut = Shortcut { key: key.name().into(), ctrl: modifiers.ctrl, alt: modifiers.alt, shift: modifiers.shift };
                    }
                    self.shortcut_capture = false;
                    child.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { .. })));
                }
            }
            position = child.input(|i| i.viewport().outer_rect.map(|rect| [rect.left(), rect.top()]));
            egui::Panel::bottom("settings-actions")
                .frame(egui::Frame::new().fill(colors.paper).inner_margin(egui::Margin::same(16)))
                .show(child, |ui| {
                    let valid = crate::languages::canonical(self.settings.provider, &self.settings.language).is_some();
                    if ui.add_enabled(valid, egui::Button::new(RichText::new("Save settings").color(Color32::WHITE).strong()).fill(BLUE).corner_radius(8.0).min_size(Vec2::new(130.0, 36.0))).clicked() { save = true; }
                });
            egui::CentralPanel::default().frame(egui::Frame::new().fill(colors.paper).inner_margin(egui::Margin::same(20))).show(child, |ui| {
                ui.label(RichText::new("Settings").size(22.0).strong().color(colors.ink));
                ui.label(RichText::new("Make the conversation your own.").color(colors.muted));
                ui.add_space(10.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Appearance");
                        ui.selectable_value(&mut self.settings.theme, Theme::Light, "Light");
                        ui.selectable_value(&mut self.settings.theme, Theme::Dark, "Dark");
                    });
                    ui.add_space(8.0); ui.separator(); ui.add_space(8.0);
                    ui.label(RichText::new("Practice").size(15.0).strong());
                    self.language_selector(ui);
                    ui.horizontal(|ui| { ui.label("Level"); egui::ComboBox::from_id_salt("level").selected_text(&self.settings.level).show_ui(ui, |ui| { for level in ["Beginner", "Intermediate", "Advanced"] { ui.selectable_value(&mut self.settings.level, level.into(), level); } }); });
                    ui.horizontal(|ui| {
                        ui.label("Speaking pace");
                        egui::ComboBox::from_id_salt("pace").selected_text(pace_label(self.settings.speed)).show_ui(ui, |ui| {
                            for (value, label) in [(0.75,"Very slow"),(0.82,"A little slower"),(1.0,"Natural"),(1.15,"A little faster"),(1.3,"Fast")] { ui.selectable_value(&mut self.settings.speed, value, label); }
                        });
                    });
                    ui.label(RichText::new("Adjusts the partner's speech while preserving voice pitch. Applies to the next conversation.").size(12.0).color(colors.muted));
                    ui.label("Your instructions");
                    ui.add(egui::TextEdit::multiline(&mut self.settings.instructions).desired_rows(3).desired_width(f32::INFINITY).hint_text("e.g. Correct my mistakes gently and ask about everyday life"));
                    ui.label(RichText::new("Added to the partner's system instructions for the next conversation.").size(12.0).color(colors.muted));
                    ui.add_space(8.0); ui.separator(); ui.add_space(8.0);
                    ui.label(RichText::new("Microphone").size(15.0).strong());
                    ui.horizontal_wrapped(|ui| {
                        ui.selectable_value(&mut self.settings.input_mode, InputMode::VoiceActivated, "Voice activated");
                        ui.selectable_value(&mut self.settings.input_mode, InputMode::PushToTalk, "Push to talk");
                    });
                    if self.settings.input_mode == InputMode::PushToTalk {
                        ui.horizontal(|ui| {
                            ui.label("Shortcut");
                            let label = if self.shortcut_capture { "Press a key combination…".to_owned() } else { self.settings.push_to_talk_shortcut.label() };
                            if ui.button(label).clicked() { self.shortcut_capture = true; }
                        });
                        ui.label(RichText::new(if self.shortcut_capture { "Press a key with optional Ctrl, Alt or Shift. Esc cancels." } else { "Hold this shortcut while the main window is focused; release to send." }).size(12.0).color(colors.muted));
                    } else {
                        self.shortcut_capture = false;
                        ui.label(RichText::new("Speech is detected automatically.").size(12.0).color(colors.muted));
                    }
                    ui.checkbox(&mut self.settings.noise_gate_enabled, "Reduce quiet background noise");
                    ui.add_enabled_ui(self.settings.noise_gate_enabled, |ui| {
                        ui.add(egui::Slider::new(&mut self.settings.noise_gate_db, -65.0..=-20.0).text("Noise threshold").suffix(" dB").step_by(1.0));
                    });
                    ui.label(RichText::new("Raise the threshold for a noisier room; too high can suppress quiet speech.").size(12.0).color(colors.muted));
                    ui.add_space(8.0); ui.separator(); ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add(egui::Slider::new(&mut self.settings.fade_seconds, 2.0..=30.0).text("Bubble lifetime").suffix(" s").step_by(1.0));
                        if ui.small_button("Reset to 8 s").clicked() { self.settings.fade_seconds = 8.0; }
                    });
                    ui.checkbox(&mut self.settings.minimal, "Hide conversation text");
                    ui.add_space(8.0); ui.separator(); ui.add_space(8.0);
                    ui.label(RichText::new("Provider").size(15.0).strong());
                    ui.horizontal(|ui| { ui.selectable_value(&mut self.settings.provider, Provider::Gemini, "Gemini Live"); ui.selectable_value(&mut self.settings.provider, Provider::OpenAi, "OpenAI Realtime"); });
                    let (url, model, voice, key) = match self.settings.provider {
                        Provider::Gemini => (&mut self.settings.gemini_url, &mut self.settings.gemini_model, &mut self.settings.gemini_voice, &mut self.gemini_key),
                        Provider::OpenAi => (&mut self.settings.openai_url, &mut self.settings.openai_model, &mut self.settings.openai_voice, &mut self.openai_key),
                    };
                    ui.label("WebSocket URL"); ui.add(egui::TextEdit::singleline(url).desired_width(f32::INFINITY));
                    ui.label("Model"); ui.add(egui::TextEdit::singleline(model).desired_width(f32::INFINITY));
                    ui.label("API key"); ui.add(egui::TextEdit::singleline(key).password(true).hint_text("Paste API key (not saved)").desired_width(f32::INFINITY));
                    ui.label("Voice");
                    egui::ComboBox::from_id_salt("voice").selected_text(format!("{}  {}", voice_symbol(self.settings.provider, voice), voice)).width(230.0).show_ui(ui, |ui| {
                        match self.settings.provider {
                            Provider::Gemini => for (name, description) in GEMINI_VOICES { ui.selectable_value(voice, (*name).to_owned(), format!("{}  {name} · {description}", voice_symbol(Provider::Gemini, name))); },
                            Provider::OpenAi => for name in OPENAI_VOICES { ui.selectable_value(voice, (*name).to_owned(), format!("{}  {name}", voice_symbol(Provider::OpenAi, name))); },
                        }
                    });
                    ui.label(RichText::new("Voice symbols describe perceived register; individual perception can vary.").size(12.0).color(colors.muted));
                    ui.label(RichText::new("Your key is sent to this URL and kept only in memory. Custom endpoints must use the selected provider's live protocol.").size(12.0).color(colors.muted));
                    ui.label(RichText::new("Provider, language, voice and pace changes apply to the next conversation.").size(12.0).color(colors.muted));
                    ui.add_space(10.0);
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
            match self.settings.save() {
                Ok(()) => {
                    close = true;
                    if self.session.is_none() {
                        self.status = "Ready when you are".into();
                        self.error = false;
                    }
                }
                Err(error) => {
                    self.status = format!("Could not save settings: {error}");
                    self.error = true;
                }
            }
        }
        if close {
            self.settings_open = false;
            self.shortcut_capture = false;
            self.save_positions();
        }
    }

    fn log_panel(&mut self, ctx: &egui::Context) {
        if !self.log_open {
            return;
        }
        let mut close = false;
        let mut selected_id: Option<String> = None;
        let mut position = None;
        let colors = palette(self.settings.theme);
        let accent = if self.settings.theme == Theme::Dark {
            Color32::from_rgb(145, 186, 255)
        } else {
            BLUE
        };
        let mut builder = ViewportBuilder::default()
            .with_title("Speeching conversation log")
            .with_inner_size([440.0, 510.0])
            .with_min_inner_size([320.0, 300.0]);
        if let Some(pos) = self.window_positions.log {
            builder = builder.with_position(pos);
        }
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("conversation-log"),
            builder,
            |child, _| {
                if child.input(|i| i.viewport().close_requested()) {
                    close = true;
                }
                position = child.input(|i| {
                    i.viewport()
                        .outer_rect
                        .map(|rect| [rect.left(), rect.top()])
                });
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::new()
                            .fill(colors.paper)
                            .inner_margin(egui::Margin::same(20)),
                    )
                    .show(child, |ui| {
                        ui.label(RichText::new("Conversation log").size(21.0).strong());
                        ui.label(RichText::new("Conversations are saved locally on this computer.").color(colors.muted));
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            let can_change = self.session.is_none();
                            if ui.add_enabled(can_change, egui::Button::new("New conversation")).clicked() { self.new_chat(); }
                            if ui.add_enabled(can_change, egui::Button::new(if self.library_open { "Close saved chats" } else { "Saved chats…" })).clicked() {
                                self.library_open = !self.library_open;
                                if self.library_open { self.refresh_library(); }
                            }
                        });
                        if self.library_open {
                            ui.add_space(5.0);
                            ui.add(egui::TextEdit::singleline(&mut self.library_query).hint_text("Search saved conversations").desired_width(f32::INFINITY));
                            if !self.library_warning.is_empty() { ui.label(RichText::new(&self.library_warning).size(12.0).color(colors.error)); }
                            let query = self.library_query.trim().to_lowercase();
                            let matching: Vec<_> = self.library.iter().filter(|chat| query.is_empty() || chat.title.to_lowercase().contains(&query) || chat.language.to_lowercase().contains(&query) || chat.sessions.iter().any(|s| s.messages.iter().any(|m| m.text.to_lowercase().contains(&query)))).collect();
                            egui::ScrollArea::vertical().id_salt("saved-chats").max_height(180.0).show(ui, |ui| {
                                if matching.is_empty() { ui.label(RichText::new("No conversations found.").color(colors.muted)); }
                                for chat in matching {
                                    let is_selected = self.selected_chat.as_ref().is_some_and(|selected| selected.id == chat.id);
                                    if ui.selectable_label(is_selected, &chat.title).clicked() { selected_id = Some(chat.id.clone()); }
                                    let voice = chat.sessions.last().map(|session| session.voice.as_str()).unwrap_or("Voice");
                                    ui.label(RichText::new(format!("{} · {} · {}", chat.language, voice, age_label(chat.updated_at))).size(11.0).color(colors.muted));
                                    ui.add_space(4.0);
                                }
                            });
                        } else if let Some(chat) = &self.selected_chat {
                            ui.label(RichText::new(format!("Selected: {}", chat.title)).size(12.0).color(accent));
                            if self.history_truncated {
                                ui.label(RichText::new("Recent messages will be sent; older messages remain in the log.").size(11.0).color(colors.muted));
                            } else {
                                ui.label(RichText::new("Previous messages will be sent to your provider when you start.").size(11.0).color(colors.muted));
                            }
                        } else {
                            ui.label(RichText::new("New conversation · choose a saved chat to continue it.").size(12.0).color(colors.muted));
                        }
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            let has_text = !self.transcript.sessions.is_empty();
                            if ui
                                .add_enabled(has_text, egui::Button::new("Copy"))
                                .clicked()
                            {
                                ui.ctx().copy_text(self.transcript.text());
                                self.status = "Conversation copied".into();
                                self.log_feedback = "Copied to clipboard".into();
                            }
                            if ui
                                .add_enabled(has_text, egui::Button::new("Download"))
                                .clicked()
                            {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_file_name("speeching-conversation.txt")
                                    .add_filter("Text file", &["txt"])
                                    .save_file()
                                {
                                    match std::fs::write(path, self.transcript.text()) {
                                        Ok(()) => {
                                            self.status = "Conversation saved".into();
                                            self.log_feedback = "Conversation saved".into();
                                        }
                                        Err(_) => {
                                            self.status = "Could not save the conversation".into();
                                            self.log_feedback =
                                                "Could not save the conversation".into();
                                            self.error = true;
                                        }
                                    }
                                }
                            }
                            if ui
                                .add_enabled(has_text, egui::Button::new("Clear"))
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
                        if !self.log_feedback.is_empty() {
                            ui.label(
                                RichText::new(&self.log_feedback)
                                    .size(12.0)
                                    .color(colors.muted),
                            );
                        }
                        ui.add_space(8.0);
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                if self.transcript.sessions.is_empty() {
                                    ui.add_space(18.0);
                                    ui.label(
                                        RichText::new("Your conversation log will appear here.")
                                            .color(colors.muted),
                                    );
                                }
                                for (index, session) in self.transcript.sessions.iter().enumerate()
                                {
                                    ui.add_space(12.0);
                                    ui.label(
                                        RichText::new(format!(
                                            "Conversation · {}",
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
                                            RichText::new(self.transcript.speaker_name(line))
                                                .strong()
                                                .color(accent),
                                        );
                                        ui.label(&line.text);
                                    }
                                }
                            });
                    });
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
            self.save_positions();
        }
    }

    fn bubble_overlay(&self, ctx: &egui::Context) {
        if self.settings.minimal {
            return;
        }
        let now = Instant::now();
        let visible = self.transcript.lines.iter().any(|line| {
            !line.historical
                && now.duration_since(line.updated).as_secs_f32() < self.settings.fade_seconds
        });
        if !visible {
            return;
        }
        let viewport = ctx.input(|input| input.viewport().clone());
        if viewport.minimized == Some(true) {
            return;
        }
        let Some(root) = viewport.outer_rect else {
            return;
        };
        let overlay_height = 250.0;
        let top = if root.top() >= 0.0 {
            (root.top() - overlay_height).max(0.0)
        } else {
            root.top() - overlay_height
        };
        let builder = ViewportBuilder::default()
            .with_title("Speeching bubbles")
            .with_inner_size([root.width().max(300.0), overlay_height])
            .with_position([root.left(), top])
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_has_shadow(false)
            .with_taskbar(false)
            .with_active(false)
            .with_mouse_passthrough(true)
            .with_always_on_top();
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("bubble-overlay"),
            builder,
            |child, _| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(Color32::TRANSPARENT))
                    .show(child, |ui| {
                        let rect = ui.max_rect().shrink2(Vec2::new(8.0, 4.0));
                        paint_bubbles(
                            ui,
                            rect,
                            &self.transcript,
                            self.settings.fade_seconds,
                            now,
                            self.settings.theme,
                        );
                    });
            },
        );
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
    fn ui(&mut self, root_ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root_ui.ctx().clone();
        if self.applied_theme != self.settings.theme {
            self.applied_theme = self.settings.theme;
            apply_theme(&ctx, self.applied_theme);
        }
        let colors = palette(self.settings.theme);
        if !self.initial_positioned {
            let viewport = ctx.input(|input| input.viewport().clone());
            if let (Some(rect), Some(monitor)) = (viewport.outer_rect, viewport.monitor_size) {
                self.initial_positioned = true;
                let bubble_space = 250.0;
                if rect.top() >= 0.0
                    && rect.top() < bubble_space
                    && monitor.y - rect.height() >= bubble_space
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new(
                        rect.left(),
                        bubble_space,
                    )));
                }
            }
        }
        self.update_events();
        let hold_to_talk = self.connected
            && !self.muted
            && self.active_input_mode == InputMode::PushToTalk
            && ctx.input(|i| {
                i.viewport().focused.unwrap_or(false)
                    && i.key_down(self.active_shortcut.key())
                    && self.active_shortcut.matches(i.modifiers)
            });
        if self.connected
            && self.active_input_mode == InputMode::PushToTalk
            && ctx.input(|i| i.viewport().focused.unwrap_or(false))
        {
            // The push-to-talk shortcut belongs to the microphone, not a focused button.
            ctx.input_mut(|i| {
                i.events.retain(|event| {
                    !matches!(event, egui::Event::Key { key, modifiers, .. } if *key == self.active_shortcut.key() && self.active_shortcut.matches(*modifiers))
                })
            });
        }
        if hold_to_talk != self.talking {
            self.talking = hold_to_talk;
            if self.connected && !self.muted {
                audio::play_cue(if hold_to_talk {
                    Cue::MicrophoneOn
                } else {
                    Cue::MicrophoneOff
                });
            }
            if let Some(session) = &self.session {
                session.talk(hold_to_talk);
            }
        }
        self.indicator.update(Instant::now(), self.levels().1);
        ctx.request_repaint_after(Duration::from_millis(33));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(colors.paper)
                    .inner_margin(egui::Margin::same(19)),
            )
            .show(root_ui, |ui| {
                self.top_bar(ui);
                let circle_size = (ui.available_height() * 0.43).clamp(125.0, 155.0);
                ui.add_space(11.0);
                self.microphone(ui, circle_size);
                if !self.settings.minimal {
                    let label = if self.error || !self.connected {
                        self.status.clone()
                    } else if self.muted {
                        "Microphone muted".into()
                    } else if self.active_input_mode == InputMode::PushToTalk && self.talking {
                        format!("Release {} to send", self.active_shortcut.label())
                    } else if self.indicator.speaking {
                        "Speaking…".into()
                    } else if self.active_input_mode == InputMode::PushToTalk {
                        format!("Hold {} to speak", self.active_shortcut.label())
                    } else {
                        "Listening…".into()
                    };
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(label).size(13.0).color(if self.error {
                            colors.error
                        } else {
                            colors.muted
                        }));
                        ui.label(
                            RichText::new(format!(
                                "{} · {}",
                                self.settings.language,
                                if self.session.is_some() {
                                    self.active_voice.as_str()
                                } else {
                                    self.settings.voice()
                                }
                            ))
                            .size(11.0)
                            .color(colors.muted),
                        );
                        if let Some(clock) = self.clock {
                            ui.label(
                                RichText::new(format_duration(clock.elapsed(Instant::now())))
                                    .size(13.0)
                                    .strong()
                                    .color(colors.ink),
                            );
                        }
                    });
                } else if self.error {
                    ui.colored_label(colors.error, &self.status);
                }
                ui.add_space(5.0);
                let wave_height = ui.available_height().clamp(30.0, 75.0);
                self.waveform(ui, wave_height);
            });
        self.bubble_overlay(&ctx);
        self.settings_panel(&ctx);
        self.log_panel(&ctx);
    }
}

fn voice_symbol(provider: Provider, voice: &str) -> &'static str {
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

fn is_modifier_key(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ControlLeft
            | egui::Key::ControlRight
            | egui::Key::ShiftLeft
            | egui::Key::ShiftRight
            | egui::Key::AltLeft
            | egui::Key::AltRight
            | egui::Key::SuperLeft
            | egui::Key::SuperRight
    )
}

fn age_label(timestamp: u64) -> String {
    let seconds = history::timestamp().saturating_sub(timestamp);
    if seconds < 60 {
        "Just now".into()
    } else if seconds < 3600 {
        format!("{} min ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{} h ago", seconds / 3600)
    } else {
        format!("{} d ago", seconds / 86_400)
    }
}

fn pace_label(speed: f32) -> &'static str {
    if speed < 0.785 {
        "Very slow"
    } else if speed < 0.96 {
        "A little slower"
    } else if speed < 1.08 {
        "Natural"
    } else if speed < 1.23 {
        "A little faster"
    } else {
        "Fast"
    }
}

fn wave_envelope(position: f32) -> f32 {
    (std::f32::consts::PI * position).sin().max(0.0).powf(0.9)
}

#[cfg(test)]
mod tests {
    use super::{is_modifier_key, wave_envelope};
    use eframe::egui;

    #[test]
    fn waveform_envelope_is_finite_at_all_drawn_points() {
        for point in 0..=72 {
            let value = wave_envelope(point as f32 / 72.0);
            assert!(value.is_finite() && value >= 0.0);
        }
    }

    #[test]
    fn shortcut_capture_waits_for_the_non_modifier_key() {
        assert!(is_modifier_key(egui::Key::ControlLeft));
        assert!(is_modifier_key(egui::Key::ShiftRight));
        assert!(!is_modifier_key(egui::Key::K));
        assert!(!is_modifier_key(egui::Key::Space));
    }
}

fn paint_bubbles(
    ui: &egui::Ui,
    rect: Rect,
    transcript: &Transcript,
    seconds: f32,
    now: Instant,
    theme: Theme,
) {
    let painter = ui.painter_at(rect);
    let mut y = rect.bottom() - 4.0;
    for line in transcript
        .lines
        .iter()
        .rev()
        .filter(|l| !l.historical && now.duration_since(l.updated).as_secs_f32() < seconds)
        .take(4)
    {
        let age = now.duration_since(line.updated).as_secs_f32();
        let fade_start = seconds * 0.7;
        let opacity = if age < fade_start {
            1.0
        } else {
            ((seconds - age) / (seconds - fade_start)).clamp(0.0, 1.0)
        };
        let alpha = (opacity * 255.0) as u8;
        let text_color = if theme == Theme::Dark {
            Color32::from_rgba_unmultiplied(232, 240, 250, alpha)
        } else {
            Color32::from_rgba_unmultiplied(24, 42, 64, alpha)
        };
        let galley = painter.layout(
            line.text.trim().to_owned(),
            FontId::proportional(13.0),
            text_color,
            (rect.width() - 55.0).max(80.0),
        );
        let height = galley.size().y + 33.0;
        y -= height + 8.0;
        if y < rect.top() {
            break;
        }
        let lift = (age / seconds).clamp(0.0, 1.0) * 10.0;
        let bubble = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, y - lift),
            Vec2::new(rect.width() - 16.0, height),
        );
        let fill = if theme == Theme::Dark && line.speaker == Speaker::User {
            Color32::from_rgba_unmultiplied(35, 57, 88, alpha)
        } else if theme == Theme::Dark {
            Color32::from_rgba_unmultiplied(39, 47, 62, alpha)
        } else if line.speaker == Speaker::User {
            Color32::from_rgba_unmultiplied(231, 240, 254, alpha)
        } else {
            Color32::from_rgba_unmultiplied(252, 253, 255, alpha)
        };
        painter.rect_filled(bubble, 12.0, fill);
        painter.rect_stroke(
            bubble,
            12.0,
            Stroke::new(
                1.0_f32,
                if theme == Theme::Dark {
                    Color32::from_rgba_unmultiplied(94, 116, 145, alpha)
                } else {
                    Color32::from_rgba_unmultiplied(176, 198, 224, alpha)
                },
            ),
            StrokeKind::Inside,
        );
        painter.text(
            bubble.left_top() + Vec2::new(12.0, 7.0),
            egui::Align2::LEFT_TOP,
            transcript.speaker_name(line),
            FontId::proportional(11.0),
            if theme == Theme::Dark {
                Color32::from_rgba_unmultiplied(156, 193, 255, alpha)
            } else {
                Color32::from_rgba_unmultiplied(42, 95, 171, alpha)
            },
        );
        painter.galley(
            bubble.left_top() + Vec2::new(12.0, 23.0),
            galley,
            text_color,
        );
    }
}

#[derive(Clone, Copy)]
enum Icon {
    Settings,
    Log,
    Eye,
    Mic,
}
fn icon_button(ui: &mut egui::Ui, icon: Icon, active: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
    let label = match icon {
        Icon::Settings => "Settings",
        Icon::Log => "Conversation log",
        Icon::Eye => "Toggle conversation text",
        Icon::Mic => "Toggle microphone",
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let painter = ui.painter();
    let dark = ui.visuals().dark_mode;
    let fill = if active {
        BLUE
    } else if dark && response.hovered() {
        Color32::from_rgb(52, 72, 99)
    } else if dark {
        Color32::from_rgb(37, 49, 66)
    } else if response.hovered() {
        Color32::from_rgb(223, 234, 249)
    } else {
        Color32::from_rgb(241, 246, 252)
    };
    painter.rect_filled(rect, 10.0, fill);
    let color = if active {
        Color32::WHITE
    } else if dark {
        Color32::from_rgb(179, 201, 232)
    } else {
        Color32::from_rgb(55, 91, 139)
    };
    let stroke = Stroke::new(1.6_f32, color);
    let c = rect.center();
    let p = |x, y| Pos2::new(c.x + x, c.y + y);
    match icon {
        Icon::Settings => {
            for y in [-6.0, 0.0, 6.0] {
                painter.line_segment([p(-8.0, y), p(8.0, y)], stroke);
            }
            for (x, y) in [(-3.0, -6.0), (4.0, 0.0), (-1.0, 6.0)] {
                painter.circle_filled(p(x, y), 2.4, fill);
                painter.circle_stroke(p(x, y), 2.4, stroke);
            }
        }
        Icon::Log => {
            for y in [-6.0, 0.0, 6.0] {
                painter.line_segment([p(-7.0, y), p(7.0, y)], stroke);
            }
        }
        Icon::Eye => {
            painter.circle_stroke(c, 7.0, stroke);
            painter.circle_filled(c, 2.0, color);
        }
        Icon::Mic => {
            painter.rect_stroke(
                Rect::from_center_size(p(0.0, -3.0), Vec2::new(7.0, 12.0)),
                3.5,
                stroke,
                StrokeKind::Inside,
            );
            painter.line_segment([p(-8.0, 2.0), p(-6.0, 6.0)], stroke);
            painter.line_segment([p(-6.0, 6.0), p(0.0, 8.0)], stroke);
            painter.line_segment([p(0.0, 8.0), p(6.0, 6.0)], stroke);
            painter.line_segment([p(6.0, 6.0), p(8.0, 2.0)], stroke);
            painter.line_segment([p(0.0, 8.0), p(0.0, 11.0)], stroke);
        }
    }
    response
}
