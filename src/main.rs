#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod app;
mod audio;
mod config;
mod engine;
mod history;
mod languages;
mod protocol;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_icon(eframe::egui::IconData {
                rgba: include_bytes!("../assets/icon.rgba").to_vec(),
                width: 64,
                height: 64,
            })
            .with_transparent(true)
            .with_inner_size([360.0, 360.0])
            .with_min_inner_size([320.0, 330.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Speeching",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
