// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

mod app;
mod image_io;
mod pixel;
mod render;
mod windowing;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, ValueEnum};

use crate::app::{ViewerApp, ViewerOptions};
use crate::image_io::load_image;
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

    let image = load_image(&cli.path)?;
    let options = ViewerOptions {
        page: cli.page,
        window_center: cli.window_center,
        window_width: cli.window_width,
        auto_window: cli.auto_window.into(),
        invert: cli.invert,
        fit: cli.fit,
    };

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        &format!("vixi {}", env!("VIXI_VERSION")),
        native_options,
        Box::new(move |cc| Ok(Box::new(ViewerApp::new(cc, image, options)))),
    )
    .map_err(|err| anyhow::anyhow!("failed to start viewer: {err}"))
}
