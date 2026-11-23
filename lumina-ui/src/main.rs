//! LuminaCalc desktop application entry point.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod i18n;
mod math_display;
mod notebook_view;
mod plot_view;
mod prog_view;
mod sci_view;
mod state;
mod style;
mod titlebar;
mod toast;
mod win_theme;

use app::LuminaApp;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LuminaCalc")
            .with_inner_size([1040.0, 680.0])
            .with_min_inner_size([680.0, 440.0])
            .with_decorations(false),
        ..Default::default()
    };

    eframe::run_native(
        "LuminaCalc",
        options,
        Box::new(|cc| Ok(Box::new(LuminaApp::new(cc)))),
    )
}
