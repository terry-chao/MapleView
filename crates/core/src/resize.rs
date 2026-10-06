//! High quality, multi-threaded downscaling.
//!
//! `fast_image_resize` picks the best SIMD kernel your CPU offers and, with the
//! `rayon` feature enabled, spreads the work across cores. Downscaling *before*
//! handing pixels to the renderer is the single cheapest performance win in an
//! image viewer: it cuts both the memory footprint and the texture upload size.

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::RgbaImage;

/// Scales `image` down so that it fits inside `max`, preserving the aspect ratio.
///
/// Returns `None` when the image already fits, so callers can keep the original
/// buffer without an extra copy.
#[must_use]
pub fn fit_rgba(image: &RgbaImage, max: (u32, u32)) -> Option<RgbaImage> {
    let target = fit_size(image.width(), image.height(), max)?;
    resize_rgba(image, target)
}

/// Computes the largest size that fits inside `max`, or `None` if no downscale
/// is needed.
#[must_use]
pub fn fit_size(width: u32, height: u32, max: (u32, u32)) -> Option<(u32, u32)> {
    if width == 0 || height == 0 || max.0 == 0 || max.1 == 0 {
        return None;
    }
    let scale = (f64::from(max.0) / f64::from(width)).min(f64::from(max.1) / f64::from(height));
    if !scale.is_finite() || scale >= 1.0 {
        return None;
    }
    let width = ((f64::from(width) * scale).round() as u32).max(1);
    let height = ((f64::from(height) * scale).round() as u32).max(1);
    Some((width, height))
}

/// Resizes to exactly `target` using a Lanczos kernel.
#[must_use]
pub fn resize_rgba(image: &RgbaImage, target: (u32, u32)) -> Option<RgbaImage> {
    if target == (image.width(), image.height()) {
        return None;
    }
    let mut target_image = RgbaImage::new(target.0.max(1), target.1.max(1));
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));
    Resizer::new()
        .resize(image, &mut target_image, &options)
        .ok()
        .map(|()| target_image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_resize_when_it_already_fits() {
        assert_eq!(fit_size(800, 600, (1920, 1080)), None);
    }

    #[test]
    fn fits_inside_the_box_preserving_aspect() {
        assert_eq!(fit_size(4000, 3000, (1000, 1000)), Some((1000, 750)));
        assert_eq!(fit_size(3000, 4000, (1000, 1000)), Some((750, 1000)));
    }

    #[test]
    fn never_produces_a_zero_dimension() {
        assert_eq!(fit_size(10_000, 3, (10, 10)), Some((10, 1)));
    }

    #[test]
    fn resizing_down_keeps_the_target_size() {
        let source = RgbaImage::new(64, 32);
        let resized = resize_rgba(&source, (16, 8)).expect("resize should produce an image");
        assert_eq!(resized.dimensions(), (16, 8));
    }
}
