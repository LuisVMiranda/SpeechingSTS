use crate::config::Theme;
use eframe::egui::{self, Color32, Vec2};

const INK: Color32 = Color32::from_rgb(25, 40, 58);
pub(super) const BLUE: Color32 = Color32::from_rgb(35, 99, 225);
const PAPER: Color32 = Color32::from_rgb(250, 252, 255);
const MUTED: Color32 = Color32::from_rgb(86, 105, 125);

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub(super) ink: Color32,
    pub(super) paper: Color32,
    pub(super) muted: Color32,
    pub(super) error: Color32,
}
pub(super) fn palette(theme: Theme) -> Palette {
    if theme == Theme::Dark {
        Palette {
            ink: Color32::from_rgb(231, 238, 247),
            paper: Color32::from_rgb(20, 27, 38),
            muted: Color32::from_rgb(162, 180, 201),
            error: Color32::from_rgb(255, 165, 150),
        }
    } else {
        Palette {
            ink: INK,
            paper: PAPER,
            muted: MUTED,
            error: Color32::from_rgb(170, 63, 52),
        }
    }
}

pub(super) fn apply_theme(ctx: &egui::Context, theme: Theme) {
    let colors = palette(theme);
    let mut style = (*ctx.global_style()).clone();
    style.visuals = if theme == Theme::Dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    style.visuals.override_text_color = Some(colors.ink);
    style.visuals.panel_fill = colors.paper;
    style.visuals.window_fill = colors.paper;
    style.visuals.widgets.noninteractive.bg_fill = colors.paper;
    style.visuals.widgets.inactive.bg_fill = if theme == Theme::Dark {
        Color32::from_rgb(38, 50, 67)
    } else {
        Color32::from_rgb(236, 242, 250)
    };
    style.visuals.widgets.hovered.bg_fill = if theme == Theme::Dark {
        Color32::from_rgb(54, 74, 101)
    } else {
        Color32::from_rgb(220, 233, 250)
    };
    style.visuals.selection.bg_fill = if theme == Theme::Dark {
        Color32::from_rgb(45, 79, 128)
    } else {
        Color32::from_rgb(169, 201, 246)
    };
    style.spacing.item_spacing = Vec2::new(8.0, 9.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.button_padding = Vec2::new(11.0, 6.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
    ctx.set_global_style(style);
}
