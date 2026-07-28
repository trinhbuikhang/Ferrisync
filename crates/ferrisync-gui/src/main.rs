//! Ferrisync desktop GUI — field-instrument style control panel for local testing.

mod app;
mod theme;
mod worker;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([800.0, 520.0])
            .with_title("Ferrisync"),
        ..Default::default()
    };

    eframe::run_native(
        "Ferrisync",
        options,
        Box::new(|cc| Ok(Box::new(app::FerrisyncApp::new(cc)))),
    )
}
