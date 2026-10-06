//! Headless tooling for MapleView.
//!
//! This exists for two reasons: it is the fastest way to check that a format
//! actually decodes, and it is the performance baseline the GUI is measured
//! against.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mapleview_core::encode::save_png;
use mapleview_core::format;
use mapleview_core::meta::human_bytes;
use mapleview_core::{DecodeHint, decode_file_with, probe_size};

#[derive(Parser)]
#[command(
    name = "mapleview-cli",
    version,
    about = "Inspect, thumbnail and benchmark images with the MapleView core"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print format, dimensions, EXIF and decode timings for one or more files.
    Info {
        #[arg(required = true)]
        paths: Vec<PathBuf>,

        /// Skip the decode and only report what the header says.
        #[arg(long)]
        header_only: bool,
    },

    /// Decode a downscaled copy and write it as PNG.
    Thumb {
        path: PathBuf,

        /// Longest edge of the result, in pixels.
        #[arg(long, short, default_value_t = 256)]
        size: u32,

        /// Output path. Defaults to `<name>.thumb.png` next to the input.
        #[arg(long, short)]
        out: Option<PathBuf>,
    },

    /// Decode the same file repeatedly and report the timings.
    Bench {
        path: PathBuf,

        #[arg(long, short, default_value_t = 10)]
        runs: u32,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Info { paths, header_only } => {
            for (index, path) in paths.iter().enumerate() {
                if index > 0 {
                    println!();
                }
                info(path, header_only)?;
            }
            Ok(())
        }
        Command::Thumb { path, size, out } => thumb(&path, size, out),
        Command::Bench { path, runs } => bench(&path, runs),
    }
}

fn info(path: &Path, header_only: bool) -> Result<()> {
    let detected = format::detect_from_file(path);
    println!("{}", path.display());
    println!("  format       {}", detected.name());
    println!(
        "  extension    {}",
        format::from_extension(path).map_or("unknown", |f| f.name())
    );

    if !detected.decodable() {
        println!(
            "  status       needs the {} codec pack",
            detected.required_backend().unwrap_or("optional")
        );
        return Ok(());
    }

    if header_only {
        let (width, height) =
            probe_size(path).with_context(|| format!("cannot read {}", path.display()))?;
        println!("  size         {width}x{height}");
        return Ok(());
    }

    let decoded = decode_file_with(path, DecodeHint::full())
        .with_context(|| format!("cannot decode {}", path.display()))?;
    let meta = &decoded.meta;
    println!("  size         {}x{}", meta.width, meta.height);
    if (meta.width, meta.height) != (meta.raw_width, meta.raw_height) {
        println!("  stored       {}x{}", meta.raw_width, meta.raw_height);
    }
    println!("  megapixels   {:.1} MP", meta.megapixels());
    println!("  orientation  {:?}", meta.orientation);
    println!("  file size    {}", human_bytes(meta.file_size));
    println!("  decode       {} ms", meta.decode_ms);
    println!("  buffer       {}", human_bytes(meta.byte_size()));

    if !meta.exif.is_empty() {
        println!("  exif");
        for (name, value) in meta.exif.iter().take(24) {
            println!("    {name:<24} {value}");
        }
        if meta.exif.len() > 24 {
            println!("    ... {} more", meta.exif.len() - 24);
        }
    }
    Ok(())
}

fn thumb(path: &Path, size: u32, out: Option<PathBuf>) -> Result<()> {
    let decoded = decode_file_with(path, DecodeHint::preview((size.max(1), size.max(1))))
        .with_context(|| format!("cannot decode {}", path.display()))?;

    let out = out.unwrap_or_else(|| {
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "thumb".to_owned());
        path.with_file_name(format!("{stem}.thumb.png"))
    });

    save_png(&decoded.image, &out).with_context(|| format!("cannot write {}", out.display()))?;
    println!(
        "{} -> {} ({}x{}, {} ms)",
        path.display(),
        out.display(),
        decoded.meta.width,
        decoded.meta.height,
        decoded.meta.decode_ms
    );
    Ok(())
}

fn bench(path: &Path, runs: u32) -> Result<()> {
    // One warm-up run so the file is in the page cache, then measure.
    let warm = decode_file_with(path, DecodeHint::full())
        .with_context(|| format!("cannot decode {}", path.display()))?;
    println!(
        "{} {}x{} {}",
        path.display(),
        warm.meta.width,
        warm.meta.height,
        human_bytes(warm.meta.file_size)
    );

    let mut timings = Vec::with_capacity(runs as usize);
    for _ in 0..runs.max(1) {
        let started = Instant::now();
        let _ = decode_file_with(path, DecodeHint::full())
            .with_context(|| format!("cannot decode {}", path.display()))?;
        timings.push(started.elapsed().as_secs_f64() * 1000.0);
    }

    timings.sort_by(f64::total_cmp);
    let min = timings[0];
    let max = timings[timings.len() - 1];
    let mean = timings.iter().sum::<f64>() / timings.len() as f64;
    let megapixels = warm.meta.megapixels();

    println!("  runs   {}", timings.len());
    println!("  min    {min:.1} ms");
    println!("  mean   {mean:.1} ms");
    println!("  max    {max:.1} ms");
    if mean > 0.0 {
        println!("  rate   {:.1} MP/s", megapixels / (mean / 1000.0));
    }
    Ok(())
}
