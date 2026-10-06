//! Encoding helpers. MapleView is a viewer, but the CLI needs to write results.

use std::path::Path;

use image::{ImageFormat, RgbaImage};

use crate::error::{Error, Result};

/// Writes an RGBA buffer as PNG.
pub fn save_png(image: &RgbaImage, path: &Path) -> Result<()> {
    image
        .save_with_format(path, ImageFormat::Png)
        .map_err(|source| Error::Encode {
            path: path.to_path_buf(),
            source,
        })
}

/// Encodes an RGBA buffer as PNG in memory.
pub fn to_png_bytes(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut buffer), ImageFormat::Png)
        .map_err(|source| Error::Encode {
            path: std::path::PathBuf::from("<memory>"),
            source,
        })?;
    Ok(buffer)
}
