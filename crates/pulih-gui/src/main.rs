// No console window behind the GUI on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod i18n;
mod preview;
mod theme;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Pulih — Recover Deleted Files")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true)
            .with_icon(theme::icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Pulih",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
