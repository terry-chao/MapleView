//! High quality, multi-threaded downscaling.
//!
//! `fast_image_resize` picks the best SIMD kernel your CPU offers and, with the
//! `rayon` feature enabled, spreads the work across cores. Downscaling *before*
//! handing pixels to the renderer is the single cheapest performance win in an
//! image viewer: it cuts both the memory footprint and the texture upload size.
//!
//! Downscaling *before* the conversion to RGBA is the second one. A 45 MP photo
//! only ever needs a couple of megapixels on screen, and the RGBA conversion is
//! a full-frame pass over every source pixel; doing it after the resize touches
//! 4% of them and never materialises a 170 MB intermediate buffer.

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{
    DynamicImage, GrayAlphaImage, GrayImage, ImageBuffer, Luma, LumaA, Rgb, Rgb32FImage, RgbImage,
    Rgba, Rgba32FImage, RgbaImage,
};

// The `image` crate only aliases the 8-bit buffers; spell the 16-bit ones out so
// the resizer can be handed a destination of the same pixel format.
type Gray16Image = ImageBuffer<Luma<u16>, Vec<u16>>;
type GrayAlpha16Image = ImageBuffer<LumaA<u16>, Vec<u16>>;
type Rgb16Image = ImageBuffer<Rgb<u16>, Vec<u16>>;
type Rgba16Image = ImageBuffer<Rgba<u16>, Vec<u16>>;

/// The kernel used for every downscale, in the GUI and the CLI alike.
fn options() -> ResizeOptions {
    ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
}

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
    Resizer::new()
        .resize(image, &mut target_image, &options())
        .ok()
        .map(|()| target_image)
}

/// Scales `image` down in the pixel format it arrived in.
///
/// This is [`fit_rgba`] without the premature RGBA conversion: an RGB8 source is
/// resized as RGB8 and only the small result is widened to RGBA. The output is
/// pixel-identical to converting first (verified by test), because the kernel is
/// applied per channel and an opaque alpha channel is the identity.
///
/// Returns `None` when the image already fits inside `max`.
#[must_use]
pub fn fit_dynamic(image: &DynamicImage, max: (u32, u32)) -> Option<DynamicImage> {
    let target = fit_size(image.width(), image.height(), max)?;
    resize_dynamic(image, target)
}

/// Resizes to exactly `target`, keeping the source's pixel format where the
/// underlying resizer supports it.
///
/// Returns `None` if the target is the current size or the resize fails, which
/// callers treat as "keep the original".
#[must_use]
pub fn resize_dynamic(image: &DynamicImage, target: (u32, u32)) -> Option<DynamicImage> {
    if target == (image.width(), image.height()) {
        return None;
    }
    let (width, height) = (target.0.max(1), target.1.max(1));
    let options = options();
    let mut resizer = Resizer::new();

    /// Resizes into a freshly allocated buffer of the same pixel type.
    macro_rules! same_format {
        ($source:expr, $empty:expr, $wrap:path) => {{
            let mut destination = $empty;
            resizer.resize($source, &mut destination, &options).ok()?;
            Some($wrap(destination))
        }};
    }

    match image {
        DynamicImage::ImageLuma8(source) => {
            same_format!(source, GrayImage::new(width, height), DynamicImage::ImageLuma8)
        }
        DynamicImage::ImageLumaA8(source) => same_format!(
            source,
            GrayAlphaImage::new(width, height),
            DynamicImage::ImageLumaA8
        ),
        DynamicImage::ImageRgb8(source) => {
            same_format!(source, RgbImage::new(width, height), DynamicImage::ImageRgb8)
        }
        DynamicImage::ImageRgba8(source) => same_format!(
            source,
            RgbaImage::new(width, height),
            DynamicImage::ImageRgba8
        ),
        DynamicImage::ImageLuma16(source) => same_format!(
            source,
            Gray16Image::new(width, height),
            DynamicImage::ImageLuma16
        ),
        DynamicImage::ImageLumaA16(source) => same_format!(
            source,
            GrayAlpha16Image::new(width, height),
            DynamicImage::ImageLumaA16
        ),
        DynamicImage::ImageRgb16(source) => same_format!(
            source,
            Rgb16Image::new(width, height),
            DynamicImage::ImageRgb16
        ),
        DynamicImage::ImageRgba16(source) => same_format!(
            source,
            Rgba16Image::new(width, height),
            DynamicImage::ImageRgba16
        ),
        DynamicImage::ImageRgb32F(source) => same_format!(
            source,
            Rgb32FImage::new(width, height),
            DynamicImage::ImageRgb32F
        ),
        DynamicImage::ImageRgba32F(source) => same_format!(
            source,
            Rgba32FImage::new(width, height),
            DynamicImage::ImageRgba32F
        ),
        // Anything else (16-bit alpha variants, exotic buffers) keeps the old
        // route: widen to RGBA8 first, then resize.
        other => resize_rgba(&other.to_rgba8(), target).map(DynamicImage::ImageRgba8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

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

    /// A source that is not already RGBA, so the two routes differ in what they
    /// carry through the kernel: RGB8 end to end, versus RGB8 widened to RGBA8
    /// first.
    fn gradient_rgb(width: u32, height: u32) -> image::RgbImage {
        image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([
                (x * 255 / width.max(1)) as u8,
                (y * 255 / height.max(1)) as u8,
                ((x * 7 + y * 13) % 256) as u8,
            ])
        })
    }

    #[test]
    fn resizing_before_the_rgba_conversion_is_pixel_identical() {
        let source = DynamicImage::ImageRgb8(gradient_rgb(96, 64));

        let early = fit_dynamic(&source, (24, 24)).expect("resize in RGB8");
        let late = fit_rgba(&source.to_rgba8(), (24, 24)).expect("resize in RGBA8");

        assert_eq!(early.dimensions(), late.dimensions());
        assert_eq!(
            early.into_rgba8().as_raw(),
            late.as_raw(),
            "converting after the resize must not change a single channel"
        );
    }

    #[test]
    fn resizing_before_conversion_keeps_an_alpha_channel_honest() {
        let mut source = DynamicImage::ImageRgba8(RgbaImage::new(64, 64));
        if let DynamicImage::ImageRgba8(image) = &mut source {
            for (x, y, pixel) in image.enumerate_pixels_mut() {
                *pixel = image::Rgba([(x * 4) as u8, (y * 4) as u8, 200, (x * 4) as u8]);
            }
        }

        let early = fit_dynamic(&source, (16, 16)).expect("resize in RGBA8");
        let late = fit_rgba(&source.to_rgba8(), (16, 16)).expect("resize in RGBA8");

        assert_eq!(early.into_rgba8().as_raw(), late.as_raw());
    }

    #[test]
    fn fit_dynamic_declines_when_the_image_already_fits() {
        let source = DynamicImage::ImageRgb8(gradient_rgb(64, 48));
        assert!(fit_dynamic(&source, (128, 128)).is_none());
    }

    #[test]
    fn fit_dynamic_covers_16_bit_sources() {
        let source = DynamicImage::ImageRgb16(Rgb16Image::from_fn(64, 32, |x, y| {
            image::Rgb([(x * 1000) as u16, (y * 2000) as u16, 7])
        }));
        let resized = fit_dynamic(&source, (16, 16)).expect("resize a 16-bit image");
        assert_eq!(resized.dimensions(), (16, 8));
        assert!(resized.into_rgba8().width() == 16);
    }
}
