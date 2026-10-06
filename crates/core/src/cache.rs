//! Byte-budgeted LRU cache for decoded images.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::decode::Decoded;

/// Cache identity of a decoded buffer.
///
/// The same file decoded at two different sizes are two distinct entries, which
/// is what lets the viewer keep a cheap preview and a full resolution copy at the
/// same time.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub path: PathBuf,
    pub target: Option<(u32, u32)>,
}

impl CacheKey {
    pub fn new(path: impl Into<PathBuf>, target: Option<(u32, u32)>) -> Self {
        Self {
            path: path.into(),
            target,
        }
    }
}

/// A concurrent cache whose capacity is measured in bytes of RGBA, not entries.
///
/// Entry-count limits are useless here: a directory of thumbnails and a directory
/// of 50 MP photos differ by three orders of magnitude in memory.
#[derive(Clone)]
pub struct ImageCache {
    inner: moka::sync::Cache<CacheKey, Arc<Decoded>>,
}

impl ImageCache {
    /// Creates a cache holding at most `max_bytes` of decoded pixels.
    pub fn new(max_bytes: u64) -> Self {
        let inner = moka::sync::Cache::builder()
            .name("mapleview-images")
            .max_capacity(max_bytes)
            .weigher(|_key: &CacheKey, value: &Arc<Decoded>| -> u32 {
                value.byte_size().min(u64::from(u32::MAX)) as u32
            })
            .build();
        Self { inner }
    }

    /// Looks up a decoded image for `path` at the given target size.
    pub fn get(&self, path: &Path, target: Option<(u32, u32)>) -> Option<Arc<Decoded>> {
        self.inner.get(&CacheKey::new(path, target))
    }

    /// Looks up any decoded buffer for `path`, preferring the requested target.
    pub fn get_any(&self, path: &Path) -> Option<Arc<Decoded>> {
        if let Some(hit) = self.inner.get(&CacheKey::new(path, None)) {
            return Some(hit);
        }
        self.inner
            .iter()
            .find(|(key, _)| key.path == path)
            .map(|(_, value)| value)
    }

    /// Inserts a decoded image, returning the shared handle that now lives in the
    /// cache so callers do not have to clone the pixels.
    pub fn insert(&self, decoded: Decoded) -> Arc<Decoded> {
        let key = decoded.key();
        let shared = Arc::new(decoded);
        self.inner.insert(key, Arc::clone(&shared));
        shared
    }

    /// Inserts an already shared buffer.
    pub fn insert_shared(&self, decoded: Arc<Decoded>) {
        self.inner.insert(decoded.key(), decoded);
    }

    /// Drops every entry for a path, whatever size it was decoded at.
    pub fn invalidate(&self, path: &Path) {
        self.inner
            .invalidate_entries_if({
                let path = path.to_path_buf();
                move |key: &CacheKey, _| key.path == path
            })
            .ok();
    }

    pub fn clear(&self) {
        self.inner.invalidate_all();
    }

    /// Bytes of decoded pixels currently resident.
    pub fn weighted_size(&self) -> u64 {
        self.inner.weighted_size()
    }

    pub fn entry_count(&self) -> u64 {
        self.inner.entry_count()
    }
}

impl std::fmt::Debug for ImageCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageCache")
            .field("entries", &self.entry_count())
            .field("weighted_size", &self.weighted_size())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::ImageMeta;
    use crate::orient::Orientation;
    use image::RgbaImage;

    fn fake_decoded(name: &str, size: u32) -> Decoded {
        let image = RgbaImage::new(size, size);
        Decoded {
            meta: ImageMeta {
                path: PathBuf::from(name),
                format: crate::format::Format::Png,
                file_size: 0,
                width: size,
                height: size,
                raw_width: size,
                raw_height: size,
                orientation: Orientation::NoTransforms,
                decode_ms: 0,
                target: None,
                resized: false,
                exif: Vec::new(),
            },
            image,
        }
    }

    #[test]
    fn round_trips_an_entry() {
        let cache = ImageCache::new(64 * 1024 * 1024);
        cache.insert(fake_decoded("a.png", 16));
        assert!(cache.get(Path::new("a.png"), None).is_some());
        assert!(cache.get(Path::new("b.png"), None).is_none());
    }

    #[test]
    fn admits_constrained_cache_and_admits_it_when_empty() {
        // Verify that the weigher is actually consulted rather than the entry count.
        let cache = ImageCache::new(100);
        let entry = fake_decoded("big.png", 100);
        let bytes = entry.byte_size();
        cache.insert(entry);
        assert_eq!(
            cache.entry_count(),
            0,
            "an oversized entry must be dropped, not counted ({bytes} bytes)"
        );
    }

    #[test]
    fn get_any_falls_back_to_a_differently_sized_entry() {
        let cache = ImageCache::new(1024 * 1024);
        let mut decoded = fake_decoded("a.png", 8);
        decoded.meta.target = Some((8, 8));
        cache.insert(decoded);
        assert!(cache.get_any(Path::new("a.png")).is_some());
    }
}
