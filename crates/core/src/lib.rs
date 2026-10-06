//! Core, GUI-free primitives for MapleView.
//!
//! Nothing in this crate knows about windowing or rendering. It is deliberately
//! reusable so that the desktop app, the CLI and the test suite all exercise the
//! same decode path.
//!
//! The decode pipeline is:
//!
//! ```text
//! read -> probe format -> decode -> EXIF orientation -> resize hint -> RGBA
//! ```
//!
//! Every stage is individually testable, and [`DecodeHint`] lets callers ask for
//! a target size so that large images never have to be materialised at full
//! resolution.

pub mod cache;
pub mod decode;
pub mod encode;
pub mod error;
pub mod format;
pub mod meta;
pub mod nav;
pub mod orient;
pub mod resize;

pub use cache::{CacheKey, ImageCache};
pub use decode::{DecodeHint, Decoded, decode_bytes, decode_file, decode_file_with, probe_size};
pub use error::{Error, Result};
pub use format::{Format, is_supported_extension};
pub use meta::ImageMeta;
pub use nav::Navigator;
pub use orient::Orientation;

/// Upper bound on the number of pixels materialised for a single decode when the
/// caller did not ask for a specific size.
///
/// 64 MP covers every consumer camera on the market while keeping the worst case
/// RGBA allocation at 256 MiB.
pub const DEFAULT_MAX_DECODE_PIXELS: u64 = 64 * 1024 * 1024;
