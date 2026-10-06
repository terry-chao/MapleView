//! MapleView: a fast, keyboard-first image viewer.

mod app;
mod fonts;
mod loader;
mod view;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    init_tracing();

    let initial = std::env::args_os().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MapleView")
            .with_app_id("mapleview")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "MapleView",
        options,
        Box::new(move |cc| Ok(Box::new(app::MapleView::new(cc, initial)))),
    )
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("mapleview=info,mapleview_core=info,warn"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
