//! MapleView: a fast, keyboard-first image viewer.

// A packaged build must not flash a console window, and neither `tracing` nor
// `eprintln!` output is any use to someone double-clicking the exe. Debug builds
// keep the console so `RUST_LOG` still works while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod brand;
mod fonts;
mod loader;
mod view;

use std::path::{Path, PathBuf};

use eframe::egui;

/// The window and taskbar icon, embedded so the binary stays self-contained.
///
/// The welcome page and About dialog draw the same bitmap, so it is shared
/// through the crate root rather than embedded twice.
pub(crate) const ICON_PNG: &[u8] = include_bytes!("../../../assets/logo/mapleview-icon-256.png");

fn main() -> eframe::Result {
    init_tracing();

    let initial = std::env::args_os().nth(1).map(PathBuf::from);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(brand::display_name())
        .with_app_id("mapleview")
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([640.0, 400.0]);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        brand::display_name(),
        options,
        Box::new(move |cc| Ok(Box::new(app::MapleView::new(cc, initial)))),
    )
}

/// Decodes the embedded icon through the same pipeline the viewer uses for
/// images, so no extra image dependency is needed for one small bitmap.
fn window_icon() -> Option<egui::IconData> {
    let decoded = mapleview_core::decode_bytes(
        ICON_PNG,
        Path::new("mapleview-icon.png"),
        mapleview_core::DecodeHint::full(),
    )
    .ok()?;
    let (width, height) = decoded.dimensions();
    Some(egui::IconData {
        rgba: decoded.image.into_raw(),
        width,
        height,
    })
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
