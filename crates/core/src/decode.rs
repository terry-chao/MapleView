//! The decode pipeline.

use std::io::Cursor;
use std::path::Path;
use std::time::Instant;

use image::{DynamicImage, ImageReader, Limits, RgbaImage};

use crate::DEFAULT_MAX_DECODE_PIXELS;
use crate::cache::CacheKey;
use crate::error::{Error, Result};
use crate::format::{self, Format};
use crate::meta::{self, ImageMeta};
use crate::orient::{self, Orientation};
use crate::resize;

/// Above this many source pixels we refuse to even try, because materialising
/// the image would need multi-gigabyte allocations. Beyond this the answer is a
/// tiled decoder, which is on the roadmap rather than in this milestone.
const HARD_MAX_SOURCE_PIXELS: u64 = 512 * 1024 * 1024;

/// What the caller wants out of a decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DecodeHint {
    /// Largest acceptable output size. `None` means "full resolution".
    pub target: Option<(u32, u32)>,
    /// Whether to read and apply the EXIF orientation tag.
    pub apply_orientation: bool,
}

impl DecodeHint {
    /// Decode at full resolution, orientation applied.
    #[must_use]
    pub const fn full() -> Self {
        Self {
            target: None,
            apply_orientation: true,
        }
    }

    /// Decode at, or below, the given size.
    #[must_use]
    pub const fn preview(max: (u32, u32)) -> Self {
        Self {
            target: Some(max),
            apply_orientation: true,
        }
    }

    /// Ignore the EXIF orientation tag.
    #[must_use]
    pub const fn without_orientation(self) -> Self {
        Self {
            apply_orientation: false,
            ..self
        }
    }

    /// The natural cache key for this hint.
    #[must_use]
    pub fn cache_key(&self, path: &Path) -> CacheKey {
        CacheKey {
            path: path.to_path_buf(),
            target: self.target,
        }
    }
}

impl Default for DecodeHint {
    fn default() -> Self {
        Self::full()
    }
}

/// A decoded image plus everything we learned while decoding it.
#[derive(Debug)]
pub struct Decoded {
    pub image: RgbaImage,
    pub meta: ImageMeta,
}

impl Decoded {
    #[must_use]
    pub fn key(&self) -> CacheKey {
        CacheKey {
            path: self.meta.path.clone(),
            target: self.meta.target,
        }
    }

    #[must_use]
    pub fn byte_size(&self) -> u64 {
        self.meta.byte_size()
    }

    #[must_use]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.image.width(), self.image.height())
    }
}

/// Decodes a file at full resolution.
pub fn decode_file(path: &Path) -> Result<Decoded> {
    decode_file_with(path, DecodeHint::full())
}

/// Reads only the header and reports the stored dimensions.
///
/// This never allocates pixel memory, which makes it the right tool for listing
/// a large directory or for deciding whether a file is worth decoding.
pub fn probe_size(path: &Path) -> Result<(u32, u32)> {
    let file = std::fs::File::open(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let reader = ImageReader::new(std::io::BufReader::new(file))
        .with_guessed_format()
        .map_err(|source| decode_io_error(path, source))?;
    reader.into_dimensions().map_err(|source| Error::Decode {
        path: path.to_path_buf(),
        source,
    })
}

/// Decodes a file according to `hint`.
pub fn decode_file_with(path: &Path, hint: DecodeHint) -> Result<Decoded> {
    let bytes = std::fs::read(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    decode_bytes(&bytes, path, hint)
}

/// Decodes an in-memory buffer. `origin` is used for error messages and metadata
/// only; the format is taken from the bytes.
pub fn decode_bytes(bytes: &[u8], origin: &Path, hint: DecodeHint) -> Result<Decoded> {
    let started = Instant::now();

    let format = format::detect(bytes);
    ensure_decodable(format, origin)?;

    let orientation = if hint.apply_orientation {
        orient::read_orientation(bytes)
    } else {
        Orientation::NoTransforms
    };

    let (source_width, source_height) = probe_dimensions(bytes, origin)?;
    let source_pixels = u64::from(source_width) * u64::from(source_height);
    if source_pixels > HARD_MAX_SOURCE_PIXELS {
        tracing::warn!(
            path = %origin.display(),
            pixels = source_pixels,
            "refusing to decode an image beyond the hard pixel limit"
        );
        return Err(Error::TooLarge {
            path: origin.to_path_buf(),
            max_pixels: HARD_MAX_SOURCE_PIXELS,
        });
    }

    // No explicit target but a huge image: derive one from the pixel budget so
    // that a 200 MP panorama degrades into a usable image instead of an
    // out-of-memory abort.
    let effective_target = hint
        .target
        .or_else(|| budget_target(source_width, source_height, DEFAULT_MAX_DECODE_PIXELS));

    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|source| decode_io_error(origin, source))?;
    reader.limits(decode_limits());
    let decoded = reader.decode().map_err(|source| Error::Decode {
        path: origin.to_path_buf(),
        source,
    })?;

    finish(
        decoded,
        bytes,
        origin,
        format,
        orientation,
        effective_target,
        started,
    )
}

fn finish(
    decoded: DynamicImage,
    bytes: &[u8],
    origin: &Path,
    format: Format,
    orientation: Orientation,
    target: Option<(u32, u32)>,
    started: Instant,
) -> Result<Decoded> {
    let raw_width = decoded.width();
    let raw_height = decoded.height();

    let decoded = orient::apply(decoded, orientation);
    let rgba = decoded.into_rgba8();

    let (image, resized) = match target.and_then(|max| resize::fit_rgba(&rgba, max)) {
        Some(smaller) => (smaller, true),
        None => (rgba, false),
    };

    let meta = ImageMeta {
        path: origin.to_path_buf(),
        format,
        file_size: bytes.len() as u64,
        width: image.width(),
        height: image.height(),
        raw_width,
        raw_height,
        orientation,
        decode_ms: started.elapsed().as_millis(),
        target,
        resized,
        exif: meta::read_exif(bytes),
    };

    tracing::debug!(
        path = %origin.display(),
        format = format.name(),
        width = meta.width,
        height = meta.height,
        resized = meta.resized,
        decode_ms = meta.decode_ms,
        "decoded"
    );

    Ok(Decoded { image, meta })
}

fn ensure_decodable(format: Format, origin: &Path) -> Result<()> {
    if format == Format::Unknown {
        return Err(Error::Unsupported(format!(
            "{}: unrecognised image container",
            origin.display()
        )));
    }
    if !format.decodable() {
        let backend = format.required_backend().unwrap_or("a codec pack");
        return Err(Error::Unsupported(format!(
            "{}: {} support is not built in yet, install the {backend} codec pack",
            origin.display(),
            format.name()
        )));
    }
    Ok(())
}

/// Reads only the header, so an oversized image is rejected before its pixels
/// are ever allocated.
fn probe_dimensions(bytes: &[u8], origin: &Path) -> Result<(u32, u32)> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|source| decode_io_error(origin, source))?;
    reader.into_dimensions().map_err(|source| Error::Decode {
        path: origin.to_path_buf(),
        source,
    })
}

fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    // The image crate defaults to 512 MiB of decoder allocations; give it a
    // little more headroom so that legitimate 50 MP photos still fit.
    limits.max_alloc = Some(1024 * 1024 * 1024);
    limits
}

/// The reader's header probing surfaces plain IO errors; keep them in the decode
/// variant so callers only have to match one error kind.
fn decode_io_error(origin: &Path, source: std::io::Error) -> Error {
    Error::Decode {
        path: origin.to_path_buf(),
        source: image::ImageError::IoError(source),
    }
}

/// The largest size within `max_pixels`, or `None` if the source already fits.
fn budget_target(width: u32, height: u32, max_pixels: u64) -> Option<(u32, u32)> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels <= max_pixels || pixels == 0 {
        return None;
    }
    let scale = (max_pixels as f64 / pixels as f64).sqrt();
    let width = ((f64::from(width) * scale).round() as u32).max(1);
    let height = ((f64::from(height) * scale).round() as u32).max(1);
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_target_is_none_for_small_images() {
        assert_eq!(budget_target(4000, 3000, DEFAULT_MAX_DECODE_PIXELS), None);
    }

    #[test]
    fn budget_target_scales_huge_images_under_the_budget() {
        let target = budget_target(20_000, 10_000, 1_000_000).expect("needs a downscale");
        assert!(u64::from(target.0) * u64::from(target.1) <= 1_000_000);
        // Aspect ratio survives the rounding.
        let ratio = f64::from(target.0) / f64::from(target.1);
        assert!((ratio - 2.0).abs() < 0.01, "ratio was {ratio}");
    }

    #[test]
    fn default_hint_is_full_resolution_with_orientation() {
        assert_eq!(DecodeHint::default(), DecodeHint::full());
        assert!(DecodeHint::default().apply_orientation);
    }
}
