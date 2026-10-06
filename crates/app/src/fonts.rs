//! Runtime CJK font loading.
//!
//! egui's built-in fonts only cover Latin and Cyrillic, so every Chinese label
//! in the UI would render as an empty box ("乱码") without help. Bundling a full
//! CJK font would add roughly 10 MB to the binary, so instead we look for one
//! that is already installed on the system and register it as a fallback.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;

/// The name our font is registered under in [`egui::FontDefinitions`].
const FONT_NAME: &str = "mapleview-cjk";

/// Registers a system CJK font as a fallback for both font families.
///
/// Safe to call once at startup: if nothing is found the default egui fonts are
/// left untouched, so the app still runs (just with boxes instead of hanzi).
pub fn install(ctx: &egui::Context) {
    let Some((path, index, bytes)) = load() else {
        tracing::warn!("no CJK font found; Chinese text may render as empty boxes");
        return;
    };

    let mut fonts = egui::FontDefinitions::default();

    let mut data = egui::FontData::from_owned(bytes);
    data.index = index;
    fonts.font_data.insert(FONT_NAME.to_owned(), Arc::new(data));

    // Appended last so the existing Latin and emoji fonts keep priority and the
    // CJK font only kicks in for glyphs they do not have.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(FONT_NAME.to_owned());
    }

    ctx.set_fonts(fonts);
    tracing::info!(font = %path.display(), "installed system CJK font");
}

/// Finds the first installed CJK font, returning its path, face index and bytes.
fn load() -> Option<(PathBuf, u32, Vec<u8>)> {
    for (path, index) in candidates() {
        if let Ok(bytes) = std::fs::read(&path) {
            return Some((path, index, bytes));
        }
    }
    None
}

/// Font files to try, in order of preference, as `(path, face index)`.
///
/// The index only matters for TrueType collections (`.ttc`), where one file
/// holds several faces; face `0` is the regular weight in every font we list.
fn candidates() -> Vec<(PathBuf, u32)> {
    // An explicit override makes it possible to work around an odd system.
    if let Some(path) = std::env::var_os("MAPLEVIEW_FONT") {
        return vec![(PathBuf::from(path), 0)];
    }

    let mut paths: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let fonts = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("Fonts");
        for name in [
            "msyh.ttc",   // Microsoft YaHei
            "msyh.ttf",   // older YaHei packaging
            "Deng.ttf",   // DengXian
            "simhei.ttf", // SimHei
            "simsun.ttc", // SimSun
        ] {
            paths.push(fonts.join(name));
        }
    }

    #[cfg(target_os = "macos")]
    {
        for path in [
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
        ] {
            paths.push(PathBuf::from(path));
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        for path in [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
            "/usr/share/fonts/truetype/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/wqy-microhei/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/arphic/uming.ttc",
        ] {
            paths.push(PathBuf::from(path));
        }
    }

    paths.into_iter().map(|path| (path, 0)).collect()
}
