//! The viewer shell.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use egui::{
    Color32, ColorImage, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2, ViewportCommand,
};
use mapleview_core::format::SUPPORTED_EXTENSIONS;
use mapleview_core::meta::human_bytes;
use mapleview_core::{DecodeHint, Decoded, ImageCache, Navigator};

use crate::loader::{Loader, Outcome, Waker};
use crate::view::{FitMode, ViewState};

/// Long edge of the preview decode, quantised to a power of two so that window
/// resizes do not invalidate the cache on every frame.
const PREVIEW_MIN_EDGE: u32 = 1024;
const PREVIEW_MAX_EDGE: u32 = 4096;

/// Zoom thresholds for switching to the full resolution texture. The gap between
/// the two values is hysteresis: without it, a zoom hovering around 1.0 would
/// re-upload the image on every frame.
const FULL_ENTER: f32 = 0.95;
const FULL_LEAVE: f32 = 0.70;

/// Decoded-pixel budget. Roughly a quarter of a typical 4 GiB machine.
const CACHE_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

/// How much of the viewport, as a fraction of its own size, is uploaded *around*
/// the visible pixels. The margin is what keeps a slow pan from re-uploading the
/// texture on every frame.
const REGION_MARGIN: f32 = 0.35;

/// Source pixels the upload rectangle is quantised to. Rounding out to a block
/// makes the sequence of rectangles during a pan finite instead of continuous.
const REGION_BLOCK: u32 = 256;

/// Above this fraction of the buffer, cropping costs more than it saves.
const REGION_WHOLE_THRESHOLD: f32 = 0.9;

/// How many images on either side of the current one to decode speculatively.
/// People browse in runs, so spending two decodes on the image after next pays
/// for itself the first time the arrow key is held down.
const PREFETCH_RADIUS: usize = 2;

/// Backdrop painted behind a photo. Kept dark so images read the same in a light
/// or dark theme, but light enough to clearly not be a rendering failure.
const VIEWER_BACKDROP: Color32 = Color32::from_gray(22);

/// Project homepage, shown in the About dialog.
const ABOUT_URL: &str = "https://terry-chao.github.io/mapleview/";

/// Author credited in the About dialog.
const ABOUT_AUTHOR: &str = "Terry";

/// The welcome page's two action buttons, and the gap between them.
const BUTTON_W: f32 = 124.0;
const BUTTON_H: f32 = 34.0;
const BUTTON_GAP: f32 = 10.0;

/// What the UI is currently waiting on.
struct Request {
    generation: u64,
    path: PathBuf,
    target: Option<(u32, u32)>,
}

/// The rectangle of the decoded buffer that currently lives in the GPU texture.
///
/// The preview tier uploads the whole buffer, because a preview is already the
/// size of the window. The full resolution tier does not: at 100% zoom a 45 MP
/// photo would otherwise become a 173 MB texture built on the UI thread, only
/// for the canvas to show a couple of megapixels of it. Uploading the visible
/// rectangle instead costs a few milliseconds and keeps the texture under the
/// driver's maximum side, which an 8000-pixel-wide panorama is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Upload {
    /// Size of the buffer this rectangle was cut from, so the rectangle can be
    /// placed inside the destination of the whole image.
    buffer: (u32, u32),
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Upload {
    fn whole(buffer: (u32, u32)) -> Self {
        Self {
            buffer,
            x: 0,
            y: 0,
            width: buffer.0.max(1),
            height: buffer.1.max(1),
        }
    }

    fn is_whole(&self) -> bool {
        self.x == 0 && self.y == 0 && self.width == self.buffer.0 && self.height == self.buffer.1
    }

    /// Whether the texture already holds everything `other` needs.
    fn covers(&self, other: &Self) -> bool {
        self.buffer == other.buffer
            && self.x <= other.x
            && self.y <= other.y
            && self.x + self.width >= other.x + other.width
            && self.y + self.height >= other.y + other.height
    }

    /// Where this rectangle lands inside `image_rect`, which is where the whole
    /// image would land.
    fn destination(&self, image_rect: Rect) -> Rect {
        let size = image_rect.size();
        let at = |value: u32, total: u32| value as f32 / total.max(1) as f32;
        Rect::from_min_max(
            image_rect.min
                + Vec2::new(
                    size.x * at(self.x, self.buffer.0),
                    size.y * at(self.y, self.buffer.1),
                ),
            image_rect.min
                + Vec2::new(
                    size.x * at(self.x + self.width, self.buffer.0),
                    size.y * at(self.y + self.height, self.buffer.1),
                ),
        )
    }
}

#[derive(Debug, Clone)]
struct Message {
    text: String,
    is_error: bool,
}

impl Message {
    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

pub struct MapleView {
    cache: ImageCache,
    loader: Loader,
    nav: Option<Navigator>,
    view: ViewState,

    displayed: Option<Arc<Decoded>>,
    texture: Option<TextureHandle>,
    texture_nearest: Option<bool>,
    /// Which part of the displayed buffer the texture holds.
    region: Option<Upload>,
    /// The brand mark, decoded once and reused by the welcome page and About.
    logo: Option<TextureHandle>,

    /// A path waiting to be loaded once the canvas size is known.
    pending_open: Option<PathBuf>,
    request: Option<Request>,
    tier_full: bool,
    /// The quantised preview size currently in use.
    ///
    /// Prefetching must use *exactly* this size, otherwise the neighbour would be
    /// decoded into a differently sized cache entry and navigating to it would
    /// miss the cache.
    preview_box: (u32, u32),

    canvas: Rect,
    canvas_ppp: f32,
    scale: f32,
    show_info: bool,
    show_help: bool,
    show_about: bool,
    fullscreen: bool,
    message: Option<Message>,
}

impl MapleView {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        crate::fonts::install(&cc.egui_ctx);

        let cache = ImageCache::new(CACHE_BUDGET_BYTES);

        let context = cc.egui_ctx.clone();
        let waker: Waker = Arc::new(move || context.request_repaint());
        let loader = Loader::new(cache.clone(), waker);

        let mut view = Self {
            cache,
            loader,
            nav: None,
            view: ViewState::default(),
            displayed: None,
            texture: None,
            texture_nearest: None,
            region: None,
            logo: load_logo(&cc.egui_ctx),
            pending_open: None,
            request: None,
            tier_full: false,
            preview_box: (PREVIEW_MIN_EDGE, PREVIEW_MIN_EDGE),
            canvas: Rect::NOTHING,
            canvas_ppp: 1.0,
            scale: 1.0,
            show_info: false,
            show_help: false,
            show_about: false,
            fullscreen: false,
            message: None,
        };

        if let Some(path) = initial {
            view.open_path(&path);
        }
        view
    }

    // ---------------------------------------------------------------- loading

    /// Opens a file or directory, replacing the current browsing session.
    fn open_path(&mut self, path: &Path) {
        match Navigator::open(path) {
            Ok(navigator) => {
                self.nav = Some(navigator);
                self.begin_current();
            }
            Err(error) => {
                self.nav = None;
                self.displayed = None;
                self.message = Some(Message::error(error.to_string()));
            }
        }
    }

    /// Starts loading whatever entry the navigator currently points at.
    fn begin_current(&mut self) {
        let Some(path) = self.nav.as_ref().and_then(Navigator::current) else {
            return;
        };
        self.begin_load(path.to_path_buf());
    }

    fn begin_load(&mut self, path: PathBuf) {
        self.view.reset();
        self.tier_full = false;
        self.request = None;
        self.message = None;
        self.displayed = None;
        self.pending_open = Some(path);
    }

    /// Steps the navigator, returning the path that should be loaded next.
    fn step(&mut self, delta: isize) -> Option<PathBuf> {
        let navigator = self.nav.as_mut()?;
        let before = navigator.index();
        navigator.step(delta);
        if navigator.index() == before {
            return None;
        }
        navigator.current().map(Path::to_path_buf)
    }

    fn goto_index(&mut self, index: usize) -> Option<PathBuf> {
        let navigator = self.nav.as_mut()?;
        navigator.goto(index).map(Path::to_path_buf)
    }

    fn issue(&mut self, path: PathBuf, target: Option<(u32, u32)>) {
        if let Some(target) = target {
            self.preview_box = target;
        }
        let hint = match target {
            Some(max) => DecodeHint::preview(max),
            None => DecodeHint::full(),
        };
        let generation = self.loader.request(path.clone(), hint);
        self.request = Some(Request {
            generation,
            path,
            target,
        });
    }

    /// Asks the background threads to warm the neighbours of the current image.
    fn prefetch_neighbours(&self) {
        let Some(navigator) = self.nav.as_ref() else {
            return;
        };
        let hint = DecodeHint::preview(self.preview_box);
        self.loader
            .prefetch(navigator.neighbours(PREFETCH_RADIUS).into_iter().map(|path| (path, hint)));
    }

    fn drain_results(&mut self, ctx: &egui::Context) {
        while let Some(outcome) = self.loader.poll() {
            self.apply_outcome(ctx, outcome);
        }
    }

    fn apply_outcome(&mut self, ctx: &egui::Context, outcome: Outcome) {
        let current = self.request.as_ref().map(|request| request.generation);
        if current != Some(outcome.generation) {
            // The user navigated away while this was decoding.
            return;
        }
        self.request = None;

        match outcome.result {
            Ok(decoded) => {
                let nearest = self.scale >= 1.0;
                let region = self.needed_upload(&decoded);
                self.upload(ctx, &decoded, nearest, region);
                self.displayed = Some(decoded);
                self.message = None;
                self.prefetch_neighbours();
            }
            Err(error) => {
                self.displayed = None;
                self.texture = None;
                self.texture_nearest = None;
                self.region = None;
                self.message = Some(Message::error(format!(
                    "{}: {error}",
                    outcome.path.display()
                )));
            }
        }
    }

    /// The part of `decoded` the canvas can show, rounded out to a whole block
    /// plus a margin so that walking around a zoomed image re-uploads rarely.
    fn needed_upload(&self, decoded: &Decoded) -> Upload {
        visible_region(
            self.canvas,
            self.canvas_ppp,
            &self.view,
            source_size(decoded),
            (
                decoded.image.width().max(1),
                decoded.image.height().max(1),
            ),
        )
    }

    fn upload(
        &mut self,
        ctx: &egui::Context,
        decoded: &Arc<Decoded>,
        nearest: bool,
        region: Upload,
    ) {
        let image = region_image(decoded, &region);
        let options = if nearest {
            TextureOptions::NEAREST
        } else {
            TextureOptions::LINEAR
        };

        match self.texture.as_mut() {
            Some(handle) => handle.set(image, options),
            None => self.texture = Some(ctx.load_texture("mapleview.image", image, options)),
        }
        self.texture_nearest = Some(nearest);
        self.region = Some(region);
    }

    // ------------------------------------------------------------------- view

    /// Chooses between the preview texture and the full resolution one.
    fn sync_quality(&mut self, decoded: &Arc<Decoded>, ppp: f32) {
        // The source is small enough that we already hold every pixel.
        if !decoded.meta.resized {
            return;
        }

        if self.tier_full {
            if self.scale < FULL_LEAVE {
                self.tier_full = false;
            }
        } else if self.scale >= FULL_ENTER {
            self.tier_full = true;
        }

        let target = if self.tier_full {
            None
        } else {
            Some(preview_box(self.canvas, ppp))
        };

        let path = decoded.meta.path.clone();
        if let Some(request) = self.request.as_ref()
            && request.path == path
            && request.target == target
        {
            return;
        }
        if decoded.meta.target == target {
            return;
        }
        self.issue(path, target);
    }

    fn zoom_by(&mut self, factor: f32) {
        let Some(decoded) = self.displayed.as_ref() else {
            return;
        };
        let source = source_size(decoded);
        let ppp = self.canvas_ppp;
        let anchor = self.canvas.center();
        self.view.zoom_at(factor, anchor, self.canvas, source, ppp);
    }

    // ------------------------------------------------------------------- input

    fn handle_keys(&mut self, ctx: &egui::Context) {
        use egui::Key;

        let keys = ctx.input(|input| Keys {
            next: input.key_pressed(Key::ArrowRight)
                || input.key_pressed(Key::ArrowDown)
                || input.key_pressed(Key::Space)
                || input.key_pressed(Key::PageDown),
            prev: input.key_pressed(Key::ArrowLeft)
                || input.key_pressed(Key::ArrowUp)
                || input.key_pressed(Key::PageUp),
            first: input.key_pressed(Key::Home),
            last: input.key_pressed(Key::End),
            zoom_in: input.key_pressed(Key::Plus) || input.key_pressed(Key::Equals),
            zoom_out: input.key_pressed(Key::Minus),
            fit: input.key_pressed(Key::F) || input.key_pressed(Key::Num0),
            actual: input.key_pressed(Key::Num1),
            toggle_info: input.key_pressed(Key::I),
            toggle_help: input.key_pressed(Key::H),
            toggle_fullscreen: input.key_pressed(Key::F11),
            escape: input.key_pressed(Key::Escape),
            open: input.modifiers.command && input.key_pressed(Key::O),
        });

        if keys.open {
            self.pick_file(ctx);
        }
        if keys.next
            && let Some(path) = self.step(1)
        {
            self.begin_load(path);
        }
        if keys.prev
            && let Some(path) = self.step(-1)
        {
            self.begin_load(path);
        }
        if keys.first
            && let Some(path) = self.goto_index(0)
        {
            self.begin_load(path);
        }
        if keys.last {
            let last = self.nav.as_ref().map(Navigator::len).unwrap_or(0);
            if last > 0
                && let Some(path) = self.goto_index(last - 1)
            {
                self.begin_load(path);
            }
        }
        if keys.zoom_in {
            self.zoom_by(1.25);
        }
        if keys.zoom_out {
            self.zoom_by(1.0 / 1.25);
        }
        if keys.fit {
            self.view.set_mode(FitMode::Fit);
        }
        if keys.actual {
            self.view.set_mode(FitMode::Actual);
        }
        if keys.toggle_info {
            self.show_info = !self.show_info;
        }
        if keys.toggle_help {
            self.show_help = !self.show_help;
        }
        if keys.toggle_fullscreen {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.fullscreen));
        }
        if keys.escape && self.fullscreen {
            self.fullscreen = false;
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        if let Some(path) = dropped.first() {
            self.open_path(path);
        }
    }

    fn pick_file(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", SUPPORTED_EXTENSIONS)
            .set_title("Open an image")
            .pick_file()
        {
            self.open_path(&path);
        }
        ctx.request_repaint();
    }

    fn pick_folder(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open a folder")
            .pick_folder()
        {
            self.open_path(&path);
        }
        ctx.request_repaint();
    }

    // -------------------------------------------------------------------- chrome

    fn toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            if ui.button("打开文件").clicked() {
                self.pick_file(ctx);
            }
            if ui.button("打开文件夹").clicked() {
                self.pick_folder(ctx);
            }

            ui.separator();

            let has_nav = self.nav.is_some();
            if ui
                .add_enabled(has_nav, egui::Button::new("◀"))
                .on_hover_text("上一张 (←)")
                .clicked()
                && let Some(path) = self.step(-1)
            {
                self.begin_load(path);
            }
            if ui
                .add_enabled(has_nav, egui::Button::new("▶"))
                .on_hover_text("下一张 (→)")
                .clicked()
                && let Some(path) = self.step(1)
            {
                self.begin_load(path);
            }

            ui.separator();

            if ui.button("适应窗口").on_hover_text("F / 0").clicked() {
                self.view.set_mode(FitMode::Fit);
            }
            if ui.button("100%").on_hover_text("1").clicked() {
                self.view.set_mode(FitMode::Actual);
            }
            if ui.button("−").on_hover_text("缩小").clicked() {
                self.zoom_by(1.0 / 1.25);
            }
            if ui.button("＋").on_hover_text("放大").clicked() {
                self.zoom_by(1.25);
            }

            ui.separator();

            ui.toggle_value(&mut self.show_info, "信息");
            ui.toggle_value(&mut self.show_help, "快捷键");
            if ui.button("关于").clicked() {
                self.show_about = true;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(decoded) = self.displayed.as_ref() {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}×{}  ·  {}  ·  {}",
                            decoded.meta.width,
                            decoded.meta.height,
                            decoded.meta.format.name(),
                            human_bytes(decoded.meta.file_size)
                        ))
                        .weak(),
                    );
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let (Some(navigator), Some(decoded)) = (self.nav.as_ref(), self.displayed.as_ref()) {
                ui.label(format!("{}/{}", navigator.index() + 1, navigator.len()));
                ui.separator();
                ui.label(decoded.meta.file_name());
                ui.separator();
                ui.label(format!("缩放 {:.0}%", self.scale * 100.0));
                ui.separator();
                ui.label(format!("解码 {} ms", decoded.meta.decode_ms));
                if decoded.meta.resized {
                    ui.separator();
                    ui.label(
                        egui::RichText::new("已降采样显示").color(Color32::from_rgb(240, 180, 90)),
                    );
                }
            } else if let Some(message) = self.message.as_ref() {
                let color = if message.is_error {
                    Color32::from_rgb(235, 110, 110)
                } else {
                    Color32::LIGHT_GRAY
                };
                ui.label(egui::RichText::new(&message.text).color(color));
            } else {
                ui.label(egui::RichText::new("把图片拖进来，或按 Ctrl+O 打开").weak());
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "缓存 {} MB / {} 张",
                        self.cache.weighted_size() / (1024 * 1024),
                        self.cache.entry_count()
                    ))
                    .weak(),
                );
            });
        });
    }

    fn info_panel(&mut self, ui: &mut egui::Ui) {
        let Some(decoded) = self.displayed.clone() else {
            ui.label("没有图片");
            return;
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("mapleview.info.file")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    let meta = &decoded.meta;
                    row(ui, "文件名", &meta.file_name());
                    row(ui, "格式", meta.format.name());
                    row(ui, "尺寸", &format!("{} × {}", meta.width, meta.height));
                    row(
                        ui,
                        "存储尺寸",
                        &format!("{} × {}", meta.raw_width, meta.raw_height),
                    );
                    row(ui, "像素", &format!("{:.1} MP", meta.megapixels()));
                    row(ui, "文件大小", &human_bytes(meta.file_size));
                    row(ui, "解码耗时", &format!("{} ms", meta.decode_ms));
                    if meta.resized {
                        row(ui, "显示缓冲", "已降采样");
                    }
                    if let Some(path) = meta.path.parent() {
                        row(ui, "目录", &path.display().to_string());
                    }
                });

            if !decoded.meta.exif.is_empty() {
                ui.add_space(10.0);
                ui.separator();
                egui::CollapsingHeader::new(format!("EXIF ({} 项)", decoded.meta.exif.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::Grid::new("mapleview.info.exif")
                            .num_columns(2)
                            .spacing([12.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                for (name, value) in &decoded.meta.exif {
                                    row(ui, name, value);
                                }
                            });
                    });
            }
        });
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }
        egui::Window::new("快捷键")
            .open(&mut self.show_help)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("mapleview.help")
                    .num_columns(2)
                    .spacing([18.0, 4.0])
                    .show(ui, |ui| {
                        for (keys, action) in KEYMAP {
                            ui.label(egui::RichText::new(*keys).monospace());
                            ui.label(*action);
                            ui.end_row();
                        }
                    });
            });
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let logo = self.logo.clone();
        egui::Window::new("关于")
            .open(&mut self.show_about)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.vertical_centered(|ui| {
                    draw_logo(ui, logo.as_ref());
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(crate::brand::display_name())
                            .size(22.0)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(format!("版本 {}", env!("CARGO_PKG_VERSION"))).weak(),
                    );
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                egui::Grid::new("mapleview.about")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("作者").weak());
                        ui.label(ABOUT_AUTHOR);
                        ui.end_row();

                        ui.label(egui::RichText::new("官网").weak());
                        ui.hyperlink_to(ABOUT_URL, ABOUT_URL);
                        ui.end_row();

                        ui.label(egui::RichText::new("许可").weak());
                        ui.label(env!("CARGO_PKG_LICENSE"));
                        ui.end_row();
                    });

                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("快、顺手、格式尽可能全的图片查看器")
                            .weak()
                            .size(12.0),
                    );
                });
            });
    }

    // ------------------------------------------------------------------- canvas

    fn canvas(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let rect = ui.available_rect_before_wrap();
        if rect.width() < 1.0 || rect.height() < 1.0 {
            return;
        }
        self.canvas = rect;
        self.canvas_ppp = ctx.pixels_per_point();

        let response = ui.allocate_rect(rect, Sense::click_and_drag());

        // A pending open can only be resolved here, because the preview size
        // depends on how large the canvas actually is.
        if let Some(path) = self.pending_open.take() {
            let target = Some(preview_box(rect, self.canvas_ppp));
            self.issue(path, target);
        }

        let Some(decoded) = self.displayed.clone() else {
            self.placeholder(ui, ctx, rect);
            return;
        };

        // Neutral backdrop behind the photo; the image is drawn on top below.
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, VIEWER_BACKDROP);

        let source = source_size(&decoded);
        let ppp = self.canvas_ppp;

        self.handle_canvas_input(ctx, &response, rect, source, ppp);

        self.scale = self.view.scale(rect, source, ppp);

        let nearest = self.scale >= 1.0;
        let needed = self.needed_upload(&decoded);
        let stale = self.texture_nearest != Some(nearest)
            || self.region.is_none_or(|region| !region.covers(&needed));
        if stale {
            self.upload(ctx, &decoded, nearest, needed);
        }

        self.sync_quality(&decoded, ppp);

        let image_rect = self.view.image_rect(rect, source, ppp);
        if let (Some(texture), Some(region)) = (self.texture.as_ref(), self.region) {
            ui.painter().image(
                texture.id(),
                region.destination(image_rect),
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        if self.request.is_some() {
            ui.put(
                Rect::from_min_size(
                    rect.left_top() + Vec2::new(12.0, 12.0),
                    Vec2::new(90.0, 24.0),
                ),
                egui::Spinner::new().size(16.0),
            );
        }
    }

    fn handle_canvas_input(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
        rect: Rect,
        source: Vec2,
        ppp: f32,
    ) {
        if response.dragged() {
            self.view.pan_by(response.drag_delta(), rect, source, ppp);
        }
        if response.double_clicked() {
            let next = if self.view.mode == FitMode::Actual {
                FitMode::Fit
            } else {
                FitMode::Actual
            };
            self.view.set_mode(next);
        }

        if !response.hovered() {
            return;
        }

        let (scroll, zoom_delta, pointer) = ctx.input(|input| {
            (
                input.smooth_scroll_delta.y,
                input.zoom_delta(),
                input.pointer.hover_pos(),
            )
        });
        let Some(pointer) = pointer else {
            return;
        };

        if (zoom_delta - 1.0).abs() > f32::EPSILON {
            self.view.zoom_at(zoom_delta, pointer, rect, source, ppp);
        } else if scroll.abs() > f32::EPSILON {
            // Exponential response so a notch feels the same at 20% and at 400%.
            let factor = (scroll * 0.003).exp();
            self.view.zoom_at(factor, pointer, rect, source, ppp);
        }
    }

    /// Fills the canvas when no image is on screen: a welcome page when nothing
    /// is open, and a spinner or error notice while a decode is in flight.
    fn placeholder(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, rect: Rect) {
        let error = self
            .message
            .as_ref()
            .filter(|message| message.is_error)
            .map(|message| message.text.clone());
        let loading = self.request.is_some();
        let pending = self.pending_name();

        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(24.0)));

        if let Some(text) = error {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::ZERO, VIEWER_BACKDROP);
            let top = ((rect.height() - 120.0) * 0.5).max(0.0);
            child.vertical_centered(|ui| {
                ui.add_space(top);
                ui.label(
                    egui::RichText::new(text)
                        .color(Color32::from_rgb(235, 110, 110))
                        .size(14.0),
                );
                ui.add_space(8.0);
                ui.label(egui::RichText::new("按 Ctrl+O 打开其他文件，或把图片拖进窗口").weak());
            });
        } else if loading {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::ZERO, VIEWER_BACKDROP);
            let top = ((rect.height() - 110.0) * 0.5).max(0.0);
            child.vertical_centered(|ui| {
                ui.add_space(top);
                ui.add(egui::Spinner::new().size(28.0));
                ui.add_space(10.0);
                if let Some(name) = pending {
                    ui.label(egui::RichText::new(name).weak());
                }
                ui.label(egui::RichText::new("正在解码…").weak());
            });
        } else {
            // Nothing open: show a welcome page instead of an empty black window.
            let background = ui.visuals().panel_fill;
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::ZERO, background);
            self.welcome_screen(&mut child, ctx, rect);
        }
    }

    fn welcome_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, rect: Rect) {
        let top = ((rect.height() - 360.0) * 0.5).max(0.0);
        let logo = self.logo.clone();
        ui.vertical_centered(|ui| {
            ui.add_space(top);
            egui::Frame::new()
                .fill(ui.visuals().window_fill)
                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                .corner_radius(14.0)
                .inner_margin(egui::Margin::symmetric(34, 28))
                .show(ui, |ui| {
                    ui.set_max_width(440.0);
                    ui.vertical_centered(|ui| {
                        draw_logo(ui, logo.as_ref());
                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new(crate::brand::display_name())
                                .size(32.0)
                                .strong(),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new("快、顺手、格式尽可能全的图片查看器")
                                .weak()
                                .size(14.0),
                        );

                        ui.add_space(22.0);
                        // `ui.horizontal` would hand the row the full card width
                        // and leave the buttons hard against the left edge. A row
                        // allocated at exactly the buttons' width is centred by
                        // the surrounding `vertical_centered`.
                        let size = Vec2::new(BUTTON_W * 2.0 + BUTTON_GAP, BUTTON_H);
                        ui.allocate_ui_with_layout(
                            size,
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = BUTTON_GAP;
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new("打开文件").size(15.0),
                                        )
                                        .min_size(Vec2::new(BUTTON_W, BUTTON_H)),
                                    )
                                    .clicked()
                                {
                                    self.pick_file(ctx);
                                }
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new("打开文件夹").size(15.0),
                                        )
                                        .min_size(Vec2::new(BUTTON_W, BUTTON_H)),
                                    )
                                    .clicked()
                                {
                                    self.pick_folder(ctx);
                                }
                            },
                        );

                        ui.add_space(20.0);
                        ui.label(
                            egui::RichText::new("把图片或文件夹直接拖进窗口，或按 Ctrl+O 打开")
                                .weak()
                                .size(13.0),
                        );
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new("← → 翻页 · 滚轮缩放 · 双击切换适应窗口 / 100%")
                                .weak()
                                .size(11.0),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("内置支持 PNG · JPEG · GIF · WebP · BMP · TIFF · AVIF 等")
                                .weak()
                                .size(11.0),
                        );
                    });
                });
        });
    }

    fn pending_name(&self) -> Option<String> {
        self.request
            .as_ref()
            .and_then(|request| request.path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
    }
}

impl eframe::App for MapleView {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // egui 0.36 hands the app a bare `Ui`; every panel is shown inside it.
        let ctx = ui.ctx().clone();

        self.drain_results(&ctx);
        self.handle_drops(&ctx);
        self.handle_keys(&ctx);

        egui::Panel::top("mapleview.toolbar").show(ui, |ui| {
            self.toolbar(ui, &ctx);
        });

        if self.show_info {
            egui::Panel::right("mapleview.info")
                .default_size(300.0)
                .show(ui, |ui| {
                    self.info_panel(ui);
                });
        }

        egui::Panel::bottom("mapleview.status").show(ui, |ui| {
            self.status_bar(ui);
        });

        egui::CentralPanel::no_frame().show(ui, |ui| {
            self.canvas(ui, &ctx);
        });

        self.help_window(&ctx);
        self.about_window(&ctx);

        // A decode can land while a request is still outstanding; keeping a
        // modest repaint cadence means the spinner animates even when the worker
        // is slow enough that nothing else wakes the UI.
        if self.request.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}

struct Keys {
    next: bool,
    prev: bool,
    first: bool,
    last: bool,
    zoom_in: bool,
    zoom_out: bool,
    fit: bool,
    actual: bool,
    toggle_info: bool,
    toggle_help: bool,
    toggle_fullscreen: bool,
    escape: bool,
    open: bool,
}

const KEYMAP: &[(&str, &str)] = &[
    ("← / →  ↑ / ↓", "上一张 / 下一张"),
    ("空格 / PageDown", "下一张"),
    ("Home / End", "第一张 / 最后一张"),
    ("滚轮", "以光标为中心缩放"),
    ("拖动", "平移"),
    ("双击", "适应窗口 ↔ 100%"),
    ("＋ / −", "放大 / 缩小"),
    ("F 或 0", "适应窗口"),
    ("1", "100% (一个像素对一个物理像素)"),
    ("I", "信息面板"),
    ("H", "本窗口"),
    ("F11", "全屏"),
    ("Esc", "退出全屏"),
    ("Ctrl+O", "打开文件"),
];

fn row(ui: &mut egui::Ui, name: &str, value: &str) {
    ui.label(egui::RichText::new(name).weak());
    ui.label(value);
    ui.end_row();
}

/// Rounds `value` down to a multiple of [`REGION_BLOCK`], clamped to `0..=limit`.
fn region_floor(value: f32, limit: u32) -> u32 {
    let block = REGION_BLOCK as f32;
    let rounded = (value.max(0.0) / block).floor() * block;
    (rounded as u32).min(limit)
}

/// Rounds `value` up to a multiple of [`REGION_BLOCK`], clamped to `0..=limit`.
fn region_ceil(value: f32, limit: u32) -> u32 {
    let block = REGION_BLOCK as f32;
    let rounded = (value.max(0.0) / block).ceil() * block;
    (rounded as u32).min(limit)
}

/// The rectangle of `buffer` that the canvas can show right now.
///
/// `source` is the image size the view geometry works in, which is the *stored*
/// size even when `buffer` is a downscaled preview of it. The result is always
/// inside the buffer, always at least one pixel, and aligned to
/// [`REGION_BLOCK`] so that a slow pan produces a finite sequence of uploads.
fn visible_region(
    canvas: Rect,
    ppp: f32,
    view: &ViewState,
    source: Vec2,
    buffer: (u32, u32),
) -> Upload {
    let whole = Upload::whole(buffer);
    let image_rect = view.image_rect(canvas, source, ppp);
    if canvas.width() < 1.0
        || canvas.height() < 1.0
        || image_rect.width() < 1.0
        || image_rect.height() < 1.0
    {
        return whole;
    }

    let visible = image_rect.intersect(canvas);
    if visible.width() < 1.0 || visible.height() < 1.0 {
        return whole;
    }

    // Everything below is in buffer pixels rather than screen points.
    let to_buffer_x = buffer.0 as f32 / image_rect.width();
    let to_buffer_y = buffer.1 as f32 / image_rect.height();
    let left = (visible.min.x - image_rect.min.x) * to_buffer_x;
    let right = (visible.max.x - image_rect.min.x) * to_buffer_x;
    let top = (visible.min.y - image_rect.min.y) * to_buffer_y;
    let bottom = (visible.max.y - image_rect.min.y) * to_buffer_y;

    let margin_x = (right - left) * REGION_MARGIN;
    let margin_y = (bottom - top) * REGION_MARGIN;
    let x0 = region_floor(left - margin_x, buffer.0.saturating_sub(1));
    let y0 = region_floor(top - margin_y, buffer.1.saturating_sub(1));
    let x1 = region_ceil(right + margin_x, buffer.0);
    let y1 = region_ceil(bottom + margin_y, buffer.1);
    let crop = Upload {
        buffer,
        x: x0,
        y: y0,
        width: x1.saturating_sub(x0).max(1),
        height: y1.saturating_sub(y0).max(1),
    };

    let covered =
        (crop.width as f32 * crop.height as f32) / (buffer.0 as f32 * buffer.1 as f32);
    if covered > REGION_WHOLE_THRESHOLD {
        whole
    } else {
        crop
    }
}

/// Copies the uploaded rectangle out of a decoded buffer, ready for the GPU.
///
/// The whole-buffer case is the common one (every preview) and borrows the
/// decoded pixels directly; only the cropped full-resolution case pays a copy,
/// and it pays it on a couple of megapixels instead of forty-five.
fn region_image(decoded: &Decoded, region: &Upload) -> ColorImage {
    let size = [region.width as usize, region.height as usize];
    if region.is_whole() {
        return ColorImage::from_rgba_unmultiplied(size, decoded.image.as_raw());
    }

    let raw = decoded.image.as_raw();
    let stride = region.width as usize * 4;
    let row_bytes = decoded.image.width() as usize * 4;
    let mut pixels = Vec::with_capacity(size[0] * size[1]);
    for y in region.y..region.y + region.height {
        let start = y as usize * row_bytes + region.x as usize * 4;
        pixels.extend(
            raw[start..start + stride]
                .chunks_exact(4)
                .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])),
        );
    }
    ColorImage::new(size, pixels)
}

/// Decodes the embedded brand mark into a texture for the welcome page and the
/// About dialog. This runs once, at startup.
fn load_logo(ctx: &egui::Context) -> Option<TextureHandle> {
    let decoded = mapleview_core::decode_bytes(
        crate::ICON_PNG,
        Path::new("mapleview-icon.png"),
        DecodeHint::full(),
    )
    .ok()?;
    let (width, height) = decoded.dimensions();
    let pixels = decoded
        .image
        .into_raw()
        .chunks_exact(4)
        .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
        .collect();
    Some(ctx.load_texture(
        "mapleview-logo",
        ColorImage::new([width as usize, height as usize], pixels),
        TextureOptions::LINEAR,
    ))
}

/// The brand mark shown by the welcome page and the About dialog.
///
/// Falls back to a hand-drawn "photo" mark -- a rounded frame with a sun and a
/// mountain -- if the bitmap ever fails to decode, so the empty state keeps an
/// identity rather than a hole.
fn draw_logo(ui: &mut egui::Ui, logo: Option<&TextureHandle>) {
    let (rect, _response) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
    if let Some(texture) = logo {
        ui.painter().image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        return;
    }

    let accent = ui.visuals().selection.bg_fill;
    let painter = ui.painter();

    let frame = Rect::from_center_size(rect.center(), Vec2::splat(56.0));
    painter.rect_filled(frame, 14.0, accent.gamma_multiply(0.30));
    painter.rect_stroke(
        frame,
        14.0,
        egui::Stroke::new(2.0, accent),
        egui::StrokeKind::Inside,
    );

    // Sun in the top-right corner.
    painter.circle_filled(rect.center() + Vec2::new(13.0, -13.0), 5.0, accent);

    // Mountain range hugging the bottom edge of the frame.
    let base = frame.bottom() - 12.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(frame.left() + 10.0, base),
            Pos2::new(rect.center().x - 4.0, base - 20.0),
            Pos2::new(rect.center().x + 8.0, base - 7.0),
            Pos2::new(rect.center().x + 15.0, base - 14.0),
            Pos2::new(frame.right() - 10.0, base),
        ],
        accent,
        egui::Stroke::NONE,
    ));
}

fn source_size(decoded: &Decoded) -> Vec2 {
    Vec2::new(
        decoded.meta.raw_width.max(1) as f32,
        decoded.meta.raw_height.max(1) as f32,
    )
}

/// The decode size to use when the whole image has to fit on screen.
///
/// Quantising to a power of two means small window resizes keep hitting the same
/// cache entry instead of re-decoding.
fn preview_box(canvas: Rect, ppp: f32) -> (u32, u32) {
    let long_edge = (canvas.width().max(canvas.height()) * ppp).ceil().max(1.0);
    let quantized = (long_edge as u32)
        .next_power_of_two()
        .clamp(PREVIEW_MIN_EDGE, PREVIEW_MAX_EDGE);
    (quantized, quantized)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The welcome page's two buttons used to hug the left edge of the card,
    /// because a bare `ui.horizontal` hands the row the whole card width. This
    /// runs a real frame headlessly and checks where the buttons actually land.
    #[test]
    fn welcome_buttons_are_centred() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
            ..Default::default()
        };

        let mut buttons = Vec::new();
        let mut centre = 0.0;
        let mut output = ctx.run_ui(input, |ui| {
            centre = ui.max_rect().center().x;
            ui.vertical_centered(|ui| {
                ui.set_max_width(440.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(BUTTON_W * 2.0 + BUTTON_GAP, BUTTON_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = BUTTON_GAP;
                        for label in ["打开文件", "打开文件夹"] {
                            let button = ui.add(
                                egui::Button::new(label).min_size(Vec2::new(BUTTON_W, BUTTON_H)),
                            );
                            buttons.push(button.rect);
                        }
                    },
                );
            });
        });
        // Nothing is driving a renderer here, so the texture uploads a real
        // frame would produce have to be discarded explicitly.
        output.textures_delta.clear();

        let middle = 0.5 * (buttons[0].min.x + buttons[1].max.x);
        assert!(
            (middle - centre).abs() <= 1.0,
            "button row centres at {middle}, but the card centres at {centre}"
        );
    }

    /// A photo far larger than the window, which is the case the region upload
    /// exists for.
    fn photo() -> (Vec2, (u32, u32)) {
        (Vec2::new(8256.0, 5504.0), (8256, 5504))
    }

    fn canvas() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))
    }

    #[test]
    fn a_zoomed_photo_uploads_a_window_instead_of_the_whole_buffer() {
        let (source, buffer) = photo();
        let view = ViewState {
            mode: FitMode::Free,
            zoom: 1.0,
            offset: Vec2::ZERO,
        };

        let region = visible_region(canvas(), 1.0, &view, source, buffer);

        let whole = buffer.0 as f32 * buffer.1 as f32;
        let uploaded = region.width as f32 * region.height as f32;
        assert!(
            uploaded < whole / 8.0,
            "uploaded {uploaded} of {whole} pixels, which is not a window"
        );
        assert_eq!(region.buffer, buffer);
        assert_eq!(region.x % REGION_BLOCK, 0);
        assert_eq!(region.y % REGION_BLOCK, 0);
        assert!(region.x + region.width <= buffer.0);
        assert!(region.y + region.height <= buffer.1);

        // The middle of the photo is on screen, so it has to be in the upload.
        assert!(region.x <= buffer.0 / 2 && region.x + region.width >= buffer.0 / 2);
        assert!(region.y <= buffer.1 / 2 && region.y + region.height >= buffer.1 / 2);
    }

    #[test]
    fn a_fitting_image_still_uploads_the_whole_buffer() {
        let (source, _) = photo();
        let view = ViewState::default();
        // A preview of the same photo: smaller than the window once fitted.
        let region = visible_region(canvas(), 1.0, &view, source, (2048, 1365));
        assert_eq!(region, Upload::whole((2048, 1365)));
    }

    #[test]
    fn panning_a_little_reuses_the_upload_but_panning_a_lot_does_not() {
        let (source, buffer) = photo();
        let view = ViewState {
            mode: FitMode::Free,
            zoom: 1.0,
            offset: Vec2::ZERO,
        };
        let first = visible_region(canvas(), 1.0, &view, source, buffer);

        let nudged = ViewState {
            offset: Vec2::new(30.0, 20.0),
            ..view
        };
        assert!(
            first.covers(&visible_region(canvas(), 1.0, &nudged, source, buffer)),
            "a nudge smaller than the margin must not cost a texture upload"
        );

        let jumped = ViewState {
            offset: Vec2::new(canvas().width(), 0.0),
            ..view
        };
        assert!(
            !first.covers(&visible_region(canvas(), 1.0, &jumped, source, buffer)),
            "moving a whole screen must bring new pixels in"
        );
    }

    #[test]
    fn a_cropped_texture_lands_where_it_was_cut_from() {
        let image_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 500.0));
        let region = Upload {
            buffer: (100, 100),
            x: 25,
            y: 50,
            width: 50,
            height: 50,
        };
        let destination = region.destination(image_rect);
        assert_eq!(destination.min, Pos2::new(250.0, 250.0));
        assert_eq!(destination.max, Pos2::new(750.0, 500.0));
    }

    #[test]
    fn a_whole_upload_still_covers_the_whole_image_rect() {
        let image_rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(800.0, 600.0));
        let region = Upload::whole((2000, 1500));
        assert_eq!(region.destination(image_rect), image_rect);
    }

    #[test]
    fn covers_is_containment_rather_than_overlap() {
        let texture = Upload {
            buffer: (100, 100),
            x: 0,
            y: 0,
            width: 50,
            height: 50,
        };
        let inside = Upload {
            width: 10,
            height: 10,
            ..texture
        };
        let overlapping = Upload {
            x: 40,
            y: 40,
            width: 20,
            height: 20,
            ..texture
        };
        assert!(texture.covers(&inside));
        assert!(!texture.covers(&overlapping));
        assert!(
            !Upload {
                buffer: (200, 200),
                ..texture
            }
            .covers(&texture),
            "a texture cut from a different buffer is never a hit"
        );
    }

    /// A decoded buffer whose every pixel says where it came from.
    fn decoded_pattern(width: u32, height: u32) -> Decoded {
        let image = image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([x as u8, y as u8, 200, 255])
        });
        Decoded {
            image,
            meta: mapleview_core::ImageMeta {
                path: PathBuf::from("pattern.png"),
                format: mapleview_core::Format::Png,
                file_size: 0,
                width,
                height,
                raw_width: width,
                raw_height: height,
                orientation: mapleview_core::Orientation::NoTransforms,
                decode_ms: 0,
                target: None,
                resized: false,
                exif: Vec::new(),
            },
        }
    }

    #[test]
    fn a_cropped_texture_carries_the_pixels_it_was_cut_from() {
        let decoded = decoded_pattern(64, 32);
        let region = Upload {
            buffer: (64, 32),
            x: 8,
            y: 4,
            width: 16,
            height: 8,
        };

        let image = region_image(&decoded, &region);

        assert_eq!(image.size, [16, 8]);
        for y in 0..8 {
            for x in 0..16 {
                let expected = Color32::from_rgba_unmultiplied(
                    (x + 8) as u8,
                    (y + 4) as u8,
                    200,
                    255,
                );
                assert_eq!(
                    image.pixels[y * 16 + x],
                    expected,
                    "pixel ({x}, {y}) of the crop is wrong"
                );
            }
        }
    }

    #[test]
    fn a_whole_texture_is_the_buffer_untouched() {
        let decoded = decoded_pattern(8, 4);
        let image = region_image(&decoded, &Upload::whole((8, 4)));

        assert_eq!(image.size, [8, 4]);
        assert_eq!(image.pixels.len(), 32);
        assert_eq!(image.pixels[0], Color32::from_rgba_unmultiplied(0, 0, 200, 255));
        assert_eq!(image.pixels[31], Color32::from_rgba_unmultiplied(7, 3, 200, 255));
    }
}
