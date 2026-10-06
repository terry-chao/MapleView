//! Content-based image format probing.
//!
//! Extensions lie: screenshots get renamed, `.jpg` files turn out to be PNGs and
//! RAW files often carry no reliable extension at all. The primary signal is
//! therefore always the magic bytes, with the extension used only as a fallback.

use std::path::Path;

/// Image containers MapleView knows how to recognise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    WebP,
    Bmp,
    Tiff,
    Tga,
    Ico,
    Qoi,
    Hdr,
    OpenExr,
    Farbfeld,
    Pnm,
    Dds,
    Avif,
    Heic,
    Jxl,
    Svg,
    Psd,
    Raw,
    Pdf,
    /// A container we recognise as media but have no image decoder for.
    Video,
    Unknown,
}

impl Format {
    /// Human readable name, shown in the info panel.
    pub fn name(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::WebP => "WebP",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
            Self::Tga => "TGA",
            Self::Ico => "ICO",
            Self::Qoi => "QOI",
            Self::Hdr => "Radiance HDR",
            Self::OpenExr => "OpenEXR",
            Self::Farbfeld => "farbfeld",
            Self::Pnm => "Netpbm",
            Self::Dds => "DDS",
            Self::Avif => "AVIF",
            Self::Heic => "HEIC/HEIF",
            Self::Jxl => "JPEG XL",
            Self::Svg => "SVG",
            Self::Psd => "Photoshop",
            Self::Raw => "RAW",
            Self::Pdf => "PDF",
            Self::Video => "video",
            Self::Unknown => "unknown",
        }
    }

    /// Whether this build can actually decode the format.
    ///
    /// Formats that are recognised but not decodable are the ones slated for the
    /// optional codec-pack backends; the UI reports them as a codec-pack
    /// suggestion instead of a generic failure.
    pub fn decodable(self) -> bool {
        match self {
            Self::Png
            | Self::Jpeg
            | Self::Gif
            | Self::WebP
            | Self::Bmp
            | Self::Tiff
            | Self::Tga
            | Self::Ico
            | Self::Qoi
            | Self::Hdr
            | Self::OpenExr
            | Self::Farbfeld
            | Self::Pnm
            | Self::Dds
            | Self::Avif => true,
            Self::Heic
            | Self::Jxl
            | Self::Svg
            | Self::Psd
            | Self::Raw
            | Self::Pdf
            | Self::Video
            | Self::Unknown => false,
        }
    }

    /// The codec pack that would unlock this container.
    pub fn required_backend(self) -> Option<&'static str> {
        match self {
            Self::Heic => Some("libheif"),
            Self::Jxl => Some("libjxl"),
            Self::Svg => Some("resvg"),
            Self::Psd => Some("psd"),
            Self::Raw => Some("libraw"),
            Self::Pdf => Some("pdfium"),
            Self::Video => Some("ffmpeg"),
            _ => None,
        }
    }
}

/// Extensions the directory scanner treats as images.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "png", "apng", "jpg", "jpeg", "jpe", "jfif", "gif", "webp", "bmp", "dib", "tif", "tiff", "tga",
    "icb", "vda", "vst", "ico", "cur", "qoi", "hdr", "exr", "ff", "pnm", "pbm", "pgm", "ppm",
    "pam", "dds", "avif", "heic", "heif", "jxl", "svg", "psd", "psb", "cr2", "cr3", "nef", "nrw",
    "arw", "srf", "sr2", "dng", "raf", "orf", "rw2", "pef", "srw", "raw", "pdf",
];

/// Whether the path looks like an image based on its extension alone.
pub fn is_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            let ext = ext.to_ascii_lowercase();
            SUPPORTED_EXTENSIONS.contains(&ext.as_str())
        })
}

/// Probe a byte prefix and classify the container.
pub fn detect(head: &[u8]) -> Format {
    if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Format::Png;
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Format::Jpeg;
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Format::Gif;
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return Format::WebP;
    }
    if head.starts_with(b"BM") {
        return Format::Bmp;
    }
    if head.starts_with(b"II\x2A\x00") || head.starts_with(b"MM\x00\x2A") {
        return Format::Tiff;
    }
    if head.starts_with(&[0x00, 0x00, 0x01, 0x00]) || head.starts_with(&[0x00, 0x00, 0x02, 0x00]) {
        return Format::Ico;
    }
    if head.starts_with(b"qoif") {
        return Format::Qoi;
    }
    if head.starts_with(b"#?RADIANCE") || head.starts_with(b"#?RGBE") {
        return Format::Hdr;
    }
    if head.starts_with(&[0x76, 0x2F, 0x31, 0x01]) {
        return Format::OpenExr;
    }
    if head.starts_with(b"farbfeld") {
        return Format::Farbfeld;
    }
    if head.starts_with(b"DDS ") {
        return Format::Dds;
    }
    if head.starts_with(b"8BPS") {
        return Format::Psd;
    }
    if head.starts_with(b"%PDF") {
        return Format::Pdf;
    }
    if head.starts_with(b"FUJIFILMCCD-RAW") {
        return Format::Raw;
    }
    if head.len() >= 2 && head[0] == b'P' && head[1].is_ascii_digit() {
        return Format::Pnm;
    }
    if head.starts_with(&[0xFF, 0x0A])
        || head.starts_with(&[0x00, 0x00, 0x00, 0x0C, b'J', b'X', b'L', b' '])
    {
        return Format::Jxl;
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        return match &head[8..12] {
            b"avif" | b"avis" => Format::Avif,
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"hevm" | b"hevs"
            | b"mif1" | b"msf1" => Format::Heic,
            b"qt  " | b"isom" | b"iso2" | b"iso5" | b"iso6" | b"mp41" | b"mp42" | b"avc1"
            | b"M4V " | b"M4A " => Format::Video,
            _ => Format::Unknown,
        };
    }
    if looks_like_svg(head) {
        return Format::Svg;
    }
    Format::Unknown
}

/// Probe a file, falling back to the extension when the content is inconclusive.
pub fn detect_from_file(path: &Path) -> Format {
    let mut buf = [0u8; 1024];
    let detected = match std::fs::File::open(path)
        .and_then(|mut file| std::io::Read::read(&mut file, &mut buf))
    {
        Ok(read) => detect(&buf[..read]),
        Err(_) => Format::Unknown,
    };
    if detected == Format::Unknown {
        from_extension(path).unwrap_or(Format::Unknown)
    } else {
        detected
    }
}

/// Classify purely from the file extension.
pub fn from_extension(path: &Path) -> Option<Format> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" | "apng" => Format::Png,
        "jpg" | "jpeg" | "jpe" | "jfif" => Format::Jpeg,
        "gif" => Format::Gif,
        "webp" => Format::WebP,
        "bmp" | "dib" => Format::Bmp,
        "tif" | "tiff" => Format::Tiff,
        "tga" | "icb" | "vda" | "vst" => Format::Tga,
        "ico" | "cur" => Format::Ico,
        "qoi" => Format::Qoi,
        "hdr" => Format::Hdr,
        "exr" => Format::OpenExr,
        "ff" => Format::Farbfeld,
        "pnm" | "pbm" | "pgm" | "ppm" | "pam" => Format::Pnm,
        "dds" => Format::Dds,
        "avif" => Format::Avif,
        "heic" | "heif" => Format::Heic,
        "jxl" => Format::Jxl,
        "svg" => Format::Svg,
        "psd" | "psb" => Format::Psd,
        "pdf" => Format::Pdf,
        "cr2" | "cr3" | "nef" | "nrw" | "arw" | "srf" | "sr2" | "dng" | "raf" | "orf" | "rw2"
        | "pef" | "srw" | "raw" => Format::Raw,
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => Format::Video,
        _ => return None,
    })
}

fn looks_like_svg(head: &[u8]) -> bool {
    let window = &head[..head.len().min(1024)];
    let lowered: Vec<u8> = window.iter().map(u8::to_ascii_lowercase).collect();
    contains(&lowered, b"<svg")
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_containers() {
        assert_eq!(
            detect(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0]),
            Format::Png
        );
        assert_eq!(detect(&[0xFF, 0xD8, 0xFF, 0xE0]), Format::Jpeg);
        assert_eq!(detect(b"GIF89a....."), Format::Gif);
        assert_eq!(detect(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Format::WebP);
        assert_eq!(detect(b"II\x2A\x00rest"), Format::Tiff);
        assert_eq!(detect(b"8BPS\x00\x01"), Format::Psd);
        assert_eq!(detect(b"%PDF-1.7"), Format::Pdf);
        assert_eq!(detect(b"\x00\x00\x00\x18ftypavif"), Format::Avif);
        assert_eq!(detect(b"\x00\x00\x00\x18ftypheic"), Format::Heic);
        assert_eq!(detect(b"\x00\x00\x00\x18ftypqt  "), Format::Video);
    }

    #[test]
    fn detects_svg_despite_declaration_and_whitespace() {
        let document = br#"<?xml version="1.0"?>
<SVG xmlns="http://www.w3.org/2000/svg"></SVG>"#;
        assert_eq!(detect(document), Format::Svg);
    }

    #[test]
    fn extension_fallback_is_case_insensitive() {
        assert_eq!(
            from_extension(Path::new("a/b/Photo.JPG")),
            Some(Format::Jpeg)
        );
        assert_eq!(from_extension(Path::new("a/b/Photo.unknownext")), None);
    }
}
