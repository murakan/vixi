// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

mod app;
mod command;
mod image_io;
mod pixel;
mod render;
mod tui;
#[cfg(feature = "video")]
mod video;
mod windowing;

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use anyhow::Result;
use clap::{Parser, ValueEnum};

use crate::app::{Content, ViewerApp, ViewerOptions};
use crate::image_io::load_image;
use crate::tui::{run_repl, ControlChannels};
use crate::windowing::AutoWindowMode;

/// File extensions handled by the video pipeline rather than the image loader.
#[cfg(feature = "video")]
const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "flv", "mpg", "mpeg", "m2v", "ts", "m2ts",
    "mts", "ogv", "3gp",
];

#[cfg(feature = "video")]
fn is_video_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|ext| VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(not(feature = "video"))]
fn is_video_path(_path: &Path) -> bool {
    false
}

const COPYRIGHT: &str = "Copyright (c) 2026 Kan Murata";
const LICENSE_NOTICE: &str = "This software is released under the MIT License, see LICENSE.";
const VERSION_BANNER: &str = concat!(
    env!("VIXI_VERSION"),
    "\n",
    "Copyright (c) 2026 Kan Murata",
    "\n",
    "This software is released under the MIT License, see LICENSE."
);

#[derive(Debug, Parser)]
#[command(author, version = env!("VIXI_VERSION"), long_version = VERSION_BANNER, about)]
struct Cli {
    /// Image path to open.
    path: PathBuf,

    /// Initial page index for multipage TIFF files. Uses zero-based indexing.
    #[arg(long, default_value_t = 0)]
    page: usize,

    /// Initial window center.
    #[arg(long)]
    window_center: Option<f32>,

    /// Initial window width.
    #[arg(long)]
    window_width: Option<f32>,

    /// Automatic windowing mode.
    #[arg(long, value_enum, default_value_t = AutoWindowArg::Percentile)]
    auto_window: AutoWindowArg,

    /// Invert displayed intensity.
    #[arg(long)]
    invert: bool,

    /// Start fitted to the window.
    #[arg(long)]
    fit: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AutoWindowArg {
    Minmax,
    Percentile,
}

impl From<AutoWindowArg> for AutoWindowMode {
    fn from(value: AutoWindowArg) -> Self {
        match value {
            AutoWindowArg::Minmax => Self::MinMax,
            AutoWindowArg::Percentile => Self::Percentile {
                low: 1.0,
                high: 99.0,
            },
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    eprintln!("vixi {}", env!("VIXI_VERSION"));
    eprintln!("{COPYRIGHT}");
    eprintln!("{LICENSE_NOTICE}");

    let options = ViewerOptions {
        window_center: cli.window_center,
        window_width: cli.window_width,
        auto_window: cli.auto_window.into(),
        invert: cli.invert,
        fit: cli.fit,
    };

    // Still images are decoded up front so any error is reported before a window
    // opens. Video is opened later inside the creation closure because it needs
    // the egui context to wake the window when frames arrive.
    let path = cli.path.clone();
    let still_image = if is_video_path(&path) {
        None
    } else {
        Some(load_image(&path)?)
    };

    // The terminal owns control: commands flow to the display window, status
    // snapshots flow back, and the window hands us its egui context so the REPL
    // can wake it on demand.
    let (command_tx, command_rx) = mpsc::channel();
    let (status_tx, status_rx) = mpsc::channel();
    let (context_tx, context_rx) = mpsc::channel();

    thread::spawn(move || {
        run_repl(ControlChannels {
            commands: command_tx,
            status: status_rx,
            context: context_rx,
        });
    });

    let page = cli.page;
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        &format!("vixi {}", env!("VIXI_VERSION")),
        native_options,
        Box::new(move |cc| {
            let _ = context_tx.send(cc.egui_ctx.clone());
            let content = build_content(still_image, &path, page, cc)
                .map_err(|err| -> Box<dyn std::error::Error + Send + Sync> { err.into() })?;
            Ok(Box::new(ViewerApp::new(
                cc, content, options, command_rx, status_tx,
            )))
        }),
    )
    .map_err(|err| anyhow::anyhow!("failed to start viewer: {err}"))
    // When the window closes, returning from `main` tears down the process and
    // the REPL thread with it (it may be parked on a blocking stdin read).
}

fn build_content(
    still_image: Option<pixel::LoadedImage>,
    path: &Path,
    page: usize,
    _cc: &eframe::CreationContext<'_>,
) -> Result<Content> {
    if let Some(image) = still_image {
        return Ok(Content::image(image, page));
    }

    #[cfg(feature = "video")]
    {
        let handle = video::VideoHandle::open(path, _cc.egui_ctx.clone())?;
        Ok(Content::video(handle, PathBuf::from(path)))
    }

    #[cfg(not(feature = "video"))]
    {
        let _ = path;
        anyhow::bail!("video support is not enabled in this build")
    }
}
