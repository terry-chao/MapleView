//! Zoom and pan state.
//!
//! Everything here is expressed in *source image pixels* rather than texture
//! pixels. That matters because the app swaps between a downscaled preview
//! texture and a full resolution one while the user zooms: if the geometry were
//! tied to the texture, every swap would make the image jump.

use eframe::egui::{Pos2, Rect, Vec2};

/// Hard limits on zoom, in physical screen pixels per source pixel.
pub const MIN_SCALE: f32 = 0.002;
pub const MAX_SCALE: f32 = 64.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    /// Scale the image so it is entirely visible.
    Fit,
    /// One source pixel per physical screen pixel.
    Actual,
    /// An explicit scale with a pan offset.
    Free,
}

#[derive(Debug, Clone, Copy)]
pub struct ViewState {
    pub mode: FitMode,
    /// Screen pixels per source pixel, only meaningful in [`FitMode::Free`].
    pub zoom: f32,
    /// Pan offset in points from the centre of the viewport.
    pub offset: Vec2,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            mode: FitMode::Fit,
            zoom: 1.0,
            offset: Vec2::ZERO,
        }
    }
}

impl ViewState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Screen pixels per source pixel.
    pub fn scale(&self, viewport: Rect, image: Vec2, ppp: f32) -> f32 {
        match self.mode {
            FitMode::Fit => fit_scale(viewport, image, ppp),
            FitMode::Actual => 1.0,
            FitMode::Free => self.zoom.clamp(MIN_SCALE, MAX_SCALE),
        }
    }

    /// Where the image lands on screen.
    pub fn image_rect(&self, viewport: Rect, image: Vec2, ppp: f32) -> Rect {
        let size_points = image * self.scale(viewport, image, ppp) / ppp.max(f32::EPSILON);
        Rect::from_center_size(viewport.center() + self.active_offset(), size_points)
    }

    /// The source pixel currently under `anchor`.
    pub fn image_point_at(&self, anchor: Pos2, viewport: Rect, image: Vec2, ppp: f32) -> Vec2 {
        let center = viewport.center() + self.active_offset();
        (anchor - center) * ppp / self.scale(viewport, image, ppp).max(f32::EPSILON)
    }

    pub fn set_mode(&mut self, mode: FitMode) {
        self.mode = mode;
        self.offset = Vec2::ZERO;
        self.zoom = 1.0;
    }

    /// Zooms by `factor`, keeping the source pixel under `anchor` in place.
    pub fn zoom_at(&mut self, factor: f32, anchor: Pos2, viewport: Rect, image: Vec2, ppp: f32) {
        let current = self.scale(viewport, image, ppp);
        let target = (current * factor).clamp(MIN_SCALE, MAX_SCALE);
        if (target / current - 1.0).abs() < 1e-4 {
            return;
        }

        let image_point = self.image_point_at(anchor, viewport, image, ppp);
        self.mode = FitMode::Free;
        self.zoom = target;
        self.offset = anchor - image_point * target / ppp - viewport.center();
        self.clamp_offset(viewport, image, ppp);
    }

    /// Drags the image by `delta` points.
    pub fn pan_by(&mut self, delta: Vec2, viewport: Rect, image: Vec2, ppp: f32) {
        if delta == Vec2::ZERO {
            return;
        }
        if self.mode != FitMode::Free {
            self.zoom = self.scale(viewport, image, ppp);
            self.mode = FitMode::Free;
        }
        self.offset += delta;
        self.clamp_offset(viewport, image, ppp);
    }

    /// Keeps the image from being dragged entirely off screen.
    pub fn clamp_offset(&mut self, viewport: Rect, image: Vec2, ppp: f32) {
        let size = image * self.scale(viewport, image, ppp) / ppp.max(f32::EPSILON);
        let slack = Vec2::new(
            ((size.x - viewport.width()) * 0.5).max(0.0),
            ((size.y - viewport.height()) * 0.5).max(0.0),
        );
        self.offset.x = self.offset.x.clamp(-slack.x, slack.x);
        self.offset.y = self.offset.y.clamp(-slack.y, slack.y);
    }

    fn active_offset(&self) -> Vec2 {
        if self.mode == FitMode::Free {
            self.offset
        } else {
            Vec2::ZERO
        }
    }
}

/// The scale at which `image` exactly fits inside `viewport`.
pub fn fit_scale(viewport: Rect, image: Vec2, ppp: f32) -> f32 {
    if image.x <= 0.0 || image.y <= 0.0 || viewport.width() <= 0.0 || viewport.height() <= 0.0 {
        return 1.0;
    }
    let physical = Vec2::new(viewport.width(), viewport.height()) * ppp;
    (physical.x / image.x)
        .min(physical.y / image.y)
        .clamp(MIN_SCALE, MAX_SCALE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    fn viewport() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))
    }

    #[test]
    fn fit_shows_the_whole_image() {
        let view = ViewState::default();
        let image = vec2(4000.0, 3000.0);
        let rect = view.image_rect(viewport(), image, 1.0);
        assert!((rect.width() - 800.0).abs() < 0.01, "{rect:?}");
        assert!((rect.height() - 600.0).abs() < 0.01, "{rect:?}");
        assert_eq!((view.scale(viewport(), image, 1.0) * 100.0).round(), 20.0);
    }

    #[test]
    fn fit_accounts_for_display_scaling() {
        let view = ViewState::default();
        let image = vec2(4000.0, 3000.0);
        // A 2x display needs twice the physical pixels for the same size.
        assert!(view.scale(viewport(), image, 2.0) > view.scale(viewport(), image, 1.0));
        let rect = view.image_rect(viewport(), image, 2.0);
        assert!((rect.width() - 800.0).abs() < 0.01, "{rect:?}");
    }

    #[test]
    fn zooming_keeps_the_point_under_the_cursor() {
        let mut view = ViewState {
            mode: FitMode::Free,
            zoom: 0.2,
            offset: Vec2::ZERO,
        };
        let image = vec2(4000.0, 3000.0);
        let anchor = Pos2::new(620.0, 180.0);

        let before = view.image_point_at(anchor, viewport(), image, 1.0);
        view.zoom_at(2.5, anchor, viewport(), image, 1.0);
        let after = view.image_point_at(anchor, viewport(), image, 1.0);

        assert!(
            (before - after).length() < 0.05,
            "anchor moved from {before:?} to {after:?}"
        );
        assert!((view.zoom - 0.5).abs() < 1e-6);
    }

    #[test]
    fn panning_cannot_push_a_fitting_image_off_screen() {
        let mut view = ViewState::default();
        let image = vec2(4000.0, 3000.0);
        view.pan_by(vec2(500.0, 500.0), viewport(), image, 1.0);
        // The image exactly fits, so there is no slack to pan into.
        assert_eq!(view.offset, Vec2::ZERO);
        assert_eq!(view.mode, FitMode::Free);
    }

    #[test]
    fn panning_is_clamped_to_the_image_edges() {
        let mut view = ViewState {
            mode: FitMode::Free,
            zoom: 1.0,
            offset: Vec2::ZERO,
        };
        let image = vec2(4000.0, 3000.0);
        view.pan_by(vec2(100_000.0, 100_000.0), viewport(), image, 1.0);
        assert!((view.offset.x - 1600.0).abs() < 0.01, "{:?}", view.offset);
        assert!((view.offset.y - 1200.0).abs() < 0.01, "{:?}", view.offset);
    }

    #[test]
    fn zoom_is_clamped_to_the_supported_range() {
        let mut view = ViewState::default();
        let image = vec2(4000.0, 3000.0);
        for _ in 0..200 {
            view.zoom_at(2.0, viewport().center(), viewport(), image, 1.0);
        }
        assert!(view.zoom <= MAX_SCALE);
        for _ in 0..400 {
            view.zoom_at(0.5, viewport().center(), viewport(), image, 1.0);
        }
        assert!(view.zoom >= MIN_SCALE);
    }
}
