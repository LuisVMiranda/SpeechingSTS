use super::*;
use eframe::egui::{FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use std::time::Instant;

pub(super) fn is_modifier_key(key: egui::Key) -> bool {
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

pub(super) fn chat_title(chat: &ChatRecord, language: UiLanguage) -> &str {
    if !chat.title_is_custom
        && chat.title == "New conversation"
        && !chat.sessions.iter().any(|s| {
            s.messages
                .iter()
                .any(|m| m.speaker == Speaker::User && !m.text.trim().is_empty())
        })
    {
        language.text("New conversation")
    } else {
        &chat.title
    }
}

pub(super) fn age_label(timestamp: u64, language: UiLanguage) -> String {
    let seconds = history::timestamp().saturating_sub(timestamp);
    if seconds < 60 {
        language.text("Just now").into()
    } else if seconds < 3600 {
        language.format("{0} min ago", &(seconds / 60).to_string())
    } else if seconds < 86_400 {
        language.format("{0} h ago", &(seconds / 3600).to_string())
    } else if seconds < 172_800 {
        language.text("1 day ago").into()
    } else {
        language.format("{0} d ago", &(seconds / 86_400).to_string())
    }
}

pub(super) fn pace_label(speed: f32) -> &'static str {
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

pub(super) fn wave_envelope(position: f32) -> f32 {
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

pub(super) struct BubbleStyle {
    pub seconds: f32,
    pub now: Instant,
    pub theme: Theme,
    pub language: UiLanguage,
}

pub(super) fn paint_bubbles(
    ui: &egui::Ui,
    rect: Rect,
    transcript: &Transcript,
    style: BubbleStyle,
) {
    let BubbleStyle {
        seconds,
        now,
        theme,
        language,
    } = style;
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
        let fill = bubble_fill(theme, line.speaker, alpha);
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
            transcript.speaker_name(line, language),
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

fn bubble_fill(theme: Theme, speaker: Speaker, alpha: u8) -> Color32 {
    let (red, green, blue) = match (theme, speaker) {
        (Theme::Dark, Speaker::User) => (35, 57, 88),
        (Theme::Dark, Speaker::Partner) => (39, 47, 62),
        (Theme::Light, Speaker::User) => (231, 240, 254),
        (Theme::Light, Speaker::Partner) => (252, 253, 255),
    };
    Color32::from_rgba_unmultiplied(red, green, blue, alpha)
}

#[derive(Clone, Copy)]
pub(super) enum Icon {
    Settings,
    Log,
    Eye,
    Mic,
}
pub(super) fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    active: bool,
    language: UiLanguage,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
    let label = match icon {
        Icon::Settings => "Settings",
        Icon::Log => "Conversation log",
        Icon::Eye => "Toggle conversation text",
        Icon::Mic => "Toggle microphone",
    };
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, language.text(label))
    });
    let painter = ui.painter();
    let (fill, color) = icon_colors(active, ui.visuals().dark_mode, response.hovered());
    painter.rect_filled(rect, 10.0, fill);
    paint_icon(painter, rect, icon, fill, color);
    response
}

fn icon_colors(active: bool, dark: bool, hovered: bool) -> (Color32, Color32) {
    let fill = if active {
        BLUE
    } else if dark && hovered {
        Color32::from_rgb(52, 72, 99)
    } else if dark {
        Color32::from_rgb(37, 49, 66)
    } else if hovered {
        Color32::from_rgb(223, 234, 249)
    } else {
        Color32::from_rgb(241, 246, 252)
    };
    let color = if active {
        Color32::WHITE
    } else if dark {
        Color32::from_rgb(179, 201, 232)
    } else {
        Color32::from_rgb(55, 91, 139)
    };
    (fill, color)
}

fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, fill: Color32, color: Color32) {
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
}
