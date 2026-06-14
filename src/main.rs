// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

mod app;
mod commands;
mod config;
mod image_io;
mod pixel;
mod plugins;
mod render;
mod tui;
mod viewer;
mod windowing;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, ValueEnum};

use crate::app::{AppState, ViewerOptions};
use crate::image_io::load_image;
use crate::viewer::{run_viewer, DisplayBackend};
use crate::windowing::AutoWindowMode;

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

    /// Window system backend. On Linux, auto prefers X11 when DISPLAY is available.
    #[arg(long, value_enum, default_value_t = BackendArg::Auto)]
    backend: BackendArg,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AutoWindowArg {
    Minmax,
    Percentile,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackendArg {
    Auto,
    Wayland,
    X11,
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

impl From<BackendArg> for DisplayBackend {
    fn from(value: BackendArg) -> Self {
        match value {
            BackendArg::Auto => Self::Auto,
            BackendArg::Wayland => Self::Wayland,
            BackendArg::X11 => Self::X11,
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

    let image = load_image(&cli.path)?;
    let options = ViewerOptions {
        page: cli.page,
        window_center: cli.window_center,
        window_width: cli.window_width,
        auto_window: cli.auto_window.into(),
        invert: cli.invert,
        fit: cli.fit,
    };

    let app = AppState::new(image, options);
    run_viewer(app, cli.backend.into())
}
