//! Image metadata: the numbers the info panel shows.

use std::path::PathBuf;

use crate::format::Format;
use crate::orient::Orientation;

/// Everything we know about a decoded image beyond its pixels.
#[derive(Debug, Clone)]
pub struct ImageMeta {
    pub path: PathBuf,
    pub format: Format,
    pub file_size: u64,
    /// Dimensions as displayed, i.e. after EXIF orientation was applied.
    pub width: u32,
    pub height: u32,
    /// Dimensions as stored in the file, before orientation.
    pub raw_width: u32,
    pub raw_height: u32,
    pub orientation: Orientation,
    /// Wall clock time spent decoding, including orientation and resize.
    pub decode_ms: u128,
    /// The size cap that was requested for this decode, if any.
    pub target: Option<(u32, u32)>,
    /// Whether the decoded buffer is smaller than the source image.
    pub resized: bool,
    pub exif: Vec<(String, String)>,
}

impl ImageMeta {
    pub fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// Bytes the RGBA buffer occupies, used as the cache weight.
    pub fn byte_size(&self) -> u64 {
        self.pixels() * 4
    }

    pub fn megapixels(&self) -> f64 {
        self.pixels() as f64 / 1_000_000.0
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn exif_value(&self, key: &str) -> Option<&str> {
        self.exif
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

/// Summarises the EXIF block into display-ready pairs.
///
/// Values are taken verbatim from the file, with units where the tag defines
/// them. The list is capped so a pathological file cannot bloat the cache entry.
pub fn read_exif(bytes: &[u8]) -> Vec<(String, String)> {
    const MAX_FIELDS: usize = 96;

    let reader = exif::Reader::new();
    let Ok(exif) = reader.read_from_container(&mut std::io::Cursor::new(bytes)) else {
        return Vec::new();
    };
    exif.fields()
        .take(MAX_FIELDS)
        .map(|field| {
            let value = field.display_value().with_unit(&exif).to_string();
            (field.tag.to_string(), value)
        })
        .collect()
}

/// Human readable byte size, e.g. `4.2 MB`.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_byte_sizes() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1024), "1.0 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MB");
    }
}
