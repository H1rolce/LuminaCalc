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

const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("LuminaCalc")
        .with_inner_size([1040.0, 680.0])
        .with_min_inner_size([680.0, 440.0])
        .with_decorations(false);

    if let Ok(icon) = eframe::icon_data::from_png_bytes(APP_ICON_PNG) {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "LuminaCalc",
        options,
        Box::new(|cc| Ok(Box::new(LuminaApp::new(cc)))),
    )
}
