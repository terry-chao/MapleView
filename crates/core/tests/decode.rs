//! End-to-end checks of the decode pipeline against real encoded files.

use std::path::Path;

use image::{Rgba, RgbaImage};
use mapleview_core::{DecodeHint, Error, Format, decode_bytes, decode_file, decode_file_with};

/// A gradient whose top-left pixel is distinctive, so orientation is observable.
fn sample(width: u32, height: u32) -> RgbaImage {
    let mut image = RgbaImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = Rgba([(x % 256) as u8, (y % 256) as u8, 64, 255]);
    }
    image
}

#[test]
fn decodes_a_png_at_full_size() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("sample.png");
    sample(200, 100).save(&path).expect("save png");

    let decoded = decode_file(&path).expect("decode");

    assert_eq!(decoded.dimensions(), (200, 100));
    assert_eq!(decoded.meta.format, Format::Png);
    assert_eq!(decoded.meta.raw_width, 200);
    assert!(!decoded.meta.resized);
    assert!(decoded.meta.file_size > 0);
    assert_eq!(
        *decoded.image.get_pixel(0, 0),
        Rgba([0, 0, 64, 255]),
        "pixels must survive the decode unchanged"
    );
}

#[test]
fn honours_the_target_size_hint() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("sample.png");
    sample(400, 200).save(&path).expect("save png");

    let decoded = decode_file_with(&path, DecodeHint::preview((100, 100))).expect("decode");

    assert_eq!(decoded.dimensions(), (100, 50));
    assert!(decoded.meta.resized);
    assert_eq!(decoded.meta.target, Some((100, 100)));
    // The metadata describes the source, not the downscaled buffer.
    assert_eq!(
        (decoded.meta.raw_width, decoded.meta.raw_height),
        (400, 200)
    );
}

#[test]
fn a_small_image_is_not_blown_up_to_meet_the_hint() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("small.png");
    sample(40, 20).save(&path).expect("save png");

    let decoded = decode_file_with(&path, DecodeHint::preview((512, 512))).expect("decode");

    assert_eq!(decoded.dimensions(), (40, 20));
    assert!(!decoded.meta.resized);
}

#[test]
fn recognises_unrelated_bytes_as_unsupported() {
    let error = decode_bytes(
        b"this is not an image",
        Path::new("hello.txt"),
        DecodeHint::full(),
    )
    .expect_err("must not decode");
    assert!(matches!(error, Error::Unsupported(_)), "got {error:?}");
}

#[test]
fn recognises_a_known_container_it_cannot_decode_yet() {
    // A minimal HEIF `ftyp` box: recognised, but the codec pack is missing.
    let heic = b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00heicmif1";
    let error = decode_bytes(heic, Path::new("photo.heic"), DecodeHint::full())
        .expect_err("must not decode");

    match error {
        Error::Unsupported(message) => {
            assert!(message.contains("HEIC/HEIF"), "message was {message}");
            assert!(message.contains("libheif"), "message was {message}");
        }
        other => panic!("expected an unsupported-format error, got {other:?}"),
    }
}

#[test]
fn truncated_files_report_an_error_instead_of_panicking() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("truncated.png");
    let mut encoded = Vec::new();
    sample(64, 64)
        .write_to(
            &mut std::io::Cursor::new(&mut encoded),
            image::ImageFormat::Png,
        )
        .expect("encode");
    std::fs::write(&path, &encoded[..encoded.len() / 3]).expect("write");

    assert!(
        decode_file(&path).is_err(),
        "a truncated file must not decode"
    );
}

#[test]
fn probe_size_reads_the_header_without_decoding() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("sample.png");
    sample(321, 123).save(&path).expect("save png");

    assert_eq!(
        mapleview_core::probe_size(&path).expect("probe"),
        (321, 123)
    );
}
