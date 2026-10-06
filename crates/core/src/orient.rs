//! EXIF orientation handling.

use std::io::Cursor;

use image::DynamicImage;
use image::metadata::Orientation as ImgOrientation;

/// EXIF orientation, re-exported so callers do not need an `image` dependency.
pub use image::metadata::Orientation;

/// Reads the orientation tag from an encoded image.
///
/// Only the EXIF-compatible containers (JPEG, TIFF, PNG, WebP, HEIF) carry this
/// tag; for everything else the identity is returned. Failures are not errors:
/// missing metadata simply means "no rotation".
pub fn read_orientation(bytes: &[u8]) -> Orientation {
    let mut cursor = Cursor::new(bytes);
    let reader = exif::Reader::new();
    let Ok(exif) = reader.read_from_container(&mut cursor) else {
        return Orientation::NoTransforms;
    };
    let Some(field) = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY) else {
        return Orientation::NoTransforms;
    };
    let Some(value) = field.value.get_uint(0) else {
        return Orientation::NoTransforms;
    };
    u8::try_from(value)
        .ok()
        .and_then(ImgOrientation::from_exif)
        .unwrap_or(Orientation::NoTransforms)
}

/// Applies the orientation, returning the corrected image.
#[must_use]
pub fn apply(image: DynamicImage, orientation: Orientation) -> DynamicImage {
    if orientation == Orientation::NoTransforms {
        return image;
    }
    let mut image = image;
    image.apply_orientation(orientation);
    image
}

/// Whether applying the orientation swaps the width and the height.
pub fn swaps_axes(orientation: Orientation) -> bool {
    matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate90_swaps_axes() {
        assert!(swaps_axes(Orientation::Rotate90));
        assert!(swaps_axes(Orientation::Rotate270FlipH));
        assert!(!swaps_axes(Orientation::Rotate180));
        assert!(!swaps_axes(Orientation::FlipHorizontal));
    }

    #[test]
    fn garbage_input_yields_identity() {
        assert_eq!(
            read_orientation(b"not an image at all"),
            Orientation::NoTransforms
        );
    }

    #[test]
    fn rotate90_on_dynamic_image_is_applied() {
        let source = DynamicImage::new_rgb8(4, 2);
        let rotated = apply(source, Orientation::Rotate90);
        assert_eq!((rotated.width(), rotated.height()), (2, 4));
    }
}
