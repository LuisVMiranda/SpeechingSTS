use super::interface_helpers::{icon_button, paint_bubbles, wave_envelope, BubbleStyle, Icon};
use super::microphone_art::{paint_facets, LOGO_BLUE};
use super::*;
use eframe::egui::{
    Align, Layout, Pos2, Rect, RichText, Sense, Stroke, Vec2, ViewportBuilder, ViewportId,
};
use std::time::{Duration, Instant};

impl App {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let colors = palette(self.settings.theme);
        let lang = self.settings.ui_language;
        ui.horizontal(|ui| {
            if self.session.is_none() {
                self.quick_chat_button(ui);
            } else if !self.settings.minimal {
                ui.label(
                    RichText::new("speeching")
                        .strong()
                        .size(16.0)
                        .color(colors.ink),
                );
            }
            if self.session.is_some() {
                self.quick_chat_open = false;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.settings_button(ui, lang);
                self.log_button(ui, lang);
                self.minimal_button(ui, lang);
                self.mute_button(ui, lang);
            });
        });
    }

    fn settings_button(&mut self, ui: &mut egui::Ui, lang: UiLanguage) {
        let hint = if self.settings_open {
            "Close settings"
        } else {
            "Open settings"
        };
        if !icon_button(ui, Icon::Settings, self.settings_open, lang)
            .on_hover_text(lang.text(hint))
            .clicked()
        {
            return;
        }
        if self.settings_open {
            self.close_settings();
        } else {
            self.settings_open = true;
        }
    }

    fn log_button(&mut self, ui: &mut egui::Ui, lang: UiLanguage) {
        let hint = if self.log_open {
            "Close conversation log"
        } else {
            "Open conversation log"
        };
        if !icon_button(ui, Icon::Log, self.log_open, lang)
            .on_hover_text(lang.text(hint))
            .clicked()
        {
            return;
        }
        self.log_open = !self.log_open;
        if !self.log_open {
            self.log_placement.reset();
            self.delete_confirm = false;
            self.rename_active = false;
            self.save_positions();
        }
    }

    fn minimal_button(&mut self, ui: &mut egui::Ui, lang: UiLanguage) {
        let hint = if self.settings.minimal {
            "Show bubbles"
        } else {
            "Hide conversation text"
        };
        if !icon_button(ui, Icon::Eye, self.settings.minimal, lang)
            .on_hover_text(lang.text(hint))
            .clicked()
        {
            return;
        }
        self.settings.minimal = !self.settings.minimal;
        if let Err(error) = self.settings.save() {
            self.status = format!("Could not save settings: {error}");
            self.error = true;
        }
    }

    fn mute_button(&mut self, ui: &mut egui::Ui, lang: UiLanguage) {
        if !self.connected {
            return;
        }
        let hint = if self.muted {
            "Unmute microphone"
        } else {
            "Mute microphone"
        };
        if !icon_button(ui, Icon::Mic, self.muted, lang)
            .on_hover_text(lang.text(hint))
            .clicked()
        {
            return;
        }
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

    fn microphone(&mut self, ui: &mut egui::Ui, size: f32) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, size), Sense::hover());
        let radius = size.min(155.0) * 0.40;
        let hit_area = Rect::from_center_size(rect.center(), Vec2::splat(radius * 2.0 + 20.0));
        let response = ui.interact(hit_area, ui.id().with("main-microphone"), Sense::click());
        let active = self.session.is_some();
        let label = self.settings.ui_language.text(if active {
            "End conversation"
        } else {
            "Start conversation"
        });
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, !self.stopping, label)
        });
        let center = rect.center();
        let p = ui.painter_at(rect);
        let (input, output) = self.levels();
        let pulse = (input * 6.0).max(output * 5.0).clamp(0.0, 1.0);
        let nudge = microphone_nudge(active, self.idle_since.elapsed());
        let (outer, inner, fill) = microphone_colors(active, response.hovered());
        let ring_center = center + Vec2::new(nudge, 0.0);
        p.circle_stroke(
            ring_center,
            radius + 9.0 + pulse * 3.0,
            Stroke::new(1.0_f32, outer),
        );
        p.circle_stroke(
            center - Vec2::new(nudge * 0.5, 0.0),
            radius + 5.0,
            Stroke::new(1.0_f32, inner),
        );
        p.circle_filled(center, radius, fill);
        paint_facets(&p, center, radius, active);
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

    pub(super) fn language_selector(&mut self, ui: &mut egui::Ui) {
        let colors = palette(self.settings.theme);
        let lang = self.settings.ui_language;
        let valid =
            crate::languages::canonical(self.settings.provider, &self.settings.language).is_some();
        ui.horizontal_wrapped(|ui| {
            ui.label(lang.text("Practice language"));
            if !valid {
                ui.label(
                    RichText::new(lang.text("Choose a supported language"))
                        .size(11.0)
                        .color(colors.error),
                );
            }
        });
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.language_input)
                .desired_width(f32::INFINITY)
                .hint_text(lang.text("Search supported languages")),
        );
        if response.gained_focus() || response.changed() {
            self.language_suggestions_open = true;
        }
        if response.changed() {
            self.settings.language = crate::languages::canonical_localized(
                self.settings.provider,
                &self.language_input,
                lang,
            )
            .map(str::to_owned)
            .unwrap_or_else(|| self.language_input.clone());
        }
        let matching = crate::languages::matching_localized(
            self.settings.provider,
            &self.language_input,
            lang,
        );
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
                        RichText::new(lang.text("No matches. Try another language."))
                            .size(12.0)
                            .color(colors.muted),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for language in matching {
                                if ui.selectable_label(false, lang.text(language)).clicked() {
                                    selected = Some(language);
                                }
                            }
                        });
                }
            });
        if let Some(language) = selected {
            self.settings.language = language.to_owned();
            self.language_input = lang.text(language).to_owned();
            open = false;
        }
        self.language_suggestions_open = open;
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
            .with_title(self.settings.ui_language.text("Speeching bubbles"))
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
                            BubbleStyle {
                                seconds: self.settings.fade_seconds,
                                now,
                                theme: self.settings.theme,
                                language: self.settings.ui_language,
                            },
                        );
                    });
            },
        );
    }
    fn position_initial_window(&mut self, ctx: &egui::Context) {
        if self.initial_positioned {
            return;
        }
        let viewport = ctx.input(|input| input.viewport().clone());
        let (Some(rect), Some(monitor)) = (viewport.outer_rect, viewport.monitor_size) else {
            return;
        };
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

    fn update_talk_input(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|input| input.viewport().focused.unwrap_or(false));
        let push_to_talk =
            self.connected && self.active_input_mode == InputMode::PushToTalk && focused;
        let hold = self.talk_shortcut_held(ctx, push_to_talk);
        if push_to_talk {
            self.consume_talk_shortcut(ctx);
        }
        if hold == self.talking {
            return;
        }
        self.talking = hold;
        if self.connected && !self.muted {
            audio::play_cue(if hold {
                Cue::MicrophoneOn
            } else {
                Cue::MicrophoneOff
            });
        }
        if let Some(session) = &self.session {
            session.talk(hold);
        }
    }

    fn talk_shortcut_held(&self, ctx: &egui::Context, push_to_talk: bool) -> bool {
        push_to_talk
            && !self.muted
            && ctx.input(|input| {
                input.key_down(self.active_shortcut.key())
                    && self.active_shortcut.matches(input.modifiers)
            })
    }

    fn consume_talk_shortcut(&self, ctx: &egui::Context) {
        // The shortcut belongs to the microphone, not a focused button.
        ctx.input_mut(|input| input.events.retain(|event| {
            !matches!(event, egui::Event::Key { key, modifiers, .. } if *key == self.active_shortcut.key() && self.active_shortcut.matches(*modifiers))
        }));
    }

    fn main_content(&mut self, ui: &mut egui::Ui) {
        let colors = palette(self.settings.theme);
        let lang = self.settings.ui_language;
        self.top_bar(ui);
        let circle_size = (ui.available_height() * 0.43).clamp(125.0, 155.0);
        ui.add_space(11.0);
        self.microphone(ui, circle_size);
        if !self.settings.minimal {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(self.status_label(lang))
                        .size(13.0)
                        .color(if self.error {
                            colors.error
                        } else {
                            colors.muted
                        }),
                );
                ui.label(
                    RichText::new(format!(
                        "{} · {}",
                        lang.text(&self.settings.language),
                        self.transcript.partner_name(lang)
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
            ui.colored_label(colors.error, lang.message(&self.status));
        }
        ui.add_space(5.0);
        let wave_height = ui.available_height().clamp(30.0, 75.0);
        self.waveform(ui, wave_height);
    }

    fn status_label(&self, lang: UiLanguage) -> String {
        if self.error || !self.connected {
            lang.message(&self.status)
        } else if self.muted {
            lang.text("Microphone muted").into()
        } else if self.active_input_mode == InputMode::PushToTalk && self.talking {
            lang.format("Release {0} to send", &self.active_shortcut.label_in(lang))
        } else if self.indicator.speaking {
            lang.text("Speaking…").into()
        } else if self.active_input_mode == InputMode::PushToTalk {
            lang.format("Hold {0} to speak", &self.active_shortcut.label_in(lang))
        } else {
            lang.text("Listening…").into()
        }
    }
}

fn microphone_nudge(active: bool, elapsed: Duration) -> f32 {
    let idle_time = elapsed.as_secs_f32() % 13.0;
    if active || !(0.7..2.1).contains(&idle_time) {
        return 0.0;
    }
    let t = idle_time - 0.7;
    (t * 17.0).sin() * (1.0 - t / 1.4) * 3.0
}

fn microphone_colors(active: bool, hovered: bool) -> (Color32, Color32, Color32) {
    let outer = if active {
        Color32::from_rgb(222, 183, 184)
    } else {
        Color32::from_rgb(196, 214, 239)
    };
    let inner = if active {
        Color32::from_rgb(196, 131, 136)
    } else {
        Color32::from_rgb(149, 180, 224)
    };
    let fill = match (active, hovered) {
        (true, true) => Color32::from_rgb(119, 39, 48),
        (true, false) => Color32::from_rgb(147, 48, 58),
        (false, true) => Color32::from_rgb(6, 72, 204),
        (false, false) => LOGO_BLUE,
    };
    (outer, inner, fill)
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|input| input.viewport().close_requested()) && !self.persist_pending_keys() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }

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
        self.position_initial_window(&ctx);
        self.update_events();
        self.update_talk_input(&ctx);
        self.indicator.update(Instant::now(), self.levels().1);
        ctx.request_repaint_after(Duration::from_millis(33));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(colors.paper)
                    .inner_margin(egui::Margin::same(19)),
            )
            .show(root_ui, |ui| self.main_content(ui));
        self.bubble_overlay(&ctx);
        self.settings_panel(&ctx);
        self.log_panel(&ctx);
    }
}
