use std::path::PathBuf;

use thiserror::Error;

/// Everything that can go wrong while reading an image.
#[derive(Debug, Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("unsupported format: {0}")]
    Unsupported(String),

    #[error("{}: {source}", path.display())]
    Decode {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },

    #[error("{}: image exceeds the {} pixel budget", path.display(), max_pixels)]
    TooLarge { path: PathBuf, max_pixels: u64 },

    #[error("{}: not a directory", path.display())]
    NotADirectory { path: PathBuf },

    #[error("{}: no supported images found", path.display())]
    EmptyDirectory { path: PathBuf },

    #[error("{}: {source}", path.display())]
    Encode {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
