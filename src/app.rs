// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::image_io::load_image;
use crate::pixel::{ImageData, LoadedImage};
use crate::plugins;
use crate::render::{rasterize_view, render_to_rgba, DisplayTransform, RgbaFrame, View};
use crate::windowing::{auto_window, AutoWindowMode, Window};

#[derive(Debug, Clone, Copy)]
pub struct ViewerOptions {
    pub page: usize,
    pub window_center: Option<f32>,
    pub window_width: Option<f32>,
    pub auto_window: AutoWindowMode,
    pub invert: bool,
    pub fit: bool,
}

#[derive(Debug, Clone)]
pub enum AppCommand {
    Brightness(f32),
    Contrast(f32),
    Fit,
    Help,
    Invert,
    Mean {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    Open(PathBuf),
    PageNext,
    PagePrev,
    PageSet(usize),
    Pan {
        dx: f32,
        dy: f32,
    },
    Pixel {
        x: u32,
        y: u32,
    },
    MoveWindow {
        dx: i32,
        dy: i32,
    },
    PositionWindow {
        x: i32,
        y: i32,
    },
    PluginInstall(PathBuf),
    PluginList,
    PluginRemove(String),
    Quit,
    Reset,
    RotateLeft,
    RotateRight,
    RunPlugin {
        name: String,
        args: Vec<String>,
    },
    SetWindow {
        center: f32,
        width: f32,
    },
    ZoomBy(f32),
    ZoomTo(f32),
}

pub struct AppState {
    image: LoadedImage,
    page: usize,
    window: Window,
    auto_window_mode: AutoWindowMode,
    invert: bool,
    transform: DisplayTransform,
    view: View,
    fit_pending: bool,
    rgba_cache: Option<RgbaFrame>,
    rgba_dirty: bool,
    frame_dirty: bool,
}

#[derive(Debug, Clone)]
pub struct AppSnapshot {
    pub title: String,
    pub status: String,
    pub statistics: String,
    pub histogram: String,
}

impl AppState {
    pub fn new(image: LoadedImage, options: ViewerOptions) -> Self {
        let page = options.page.min(image.pages.len().saturating_sub(1));
        let auto = auto_window(&image.pages[page].data, options.auto_window);
        let window = Window::new(
            options.window_center.unwrap_or(auto.center),
            options.window_width.unwrap_or(auto.width),
        );
        let (width, height) = image.pages[page].data.dimensions();

        Self {
            image,
            page,
            window,
            auto_window_mode: options.auto_window,
            invert: options.invert,
            transform: DisplayTransform::default(),
            view: View {
                center_x: width as f32 / 2.0,
                center_y: height as f32 / 2.0,
                zoom: 1.0,
            },
            fit_pending: options.fit,
            rgba_cache: None,
            rgba_dirty: true,
            frame_dirty: true,
        }
    }

    pub fn title(&self) -> String {
        format!(
            "vixi - {} [{}/{}]",
            self.image.path.display(),
            self.page + 1,
            self.image.pages.len()
        )
    }

    pub fn status(&self) -> String {
        let image = self.current_image();
        let (width, height) = image.dimensions();
        format!(
            "{}x{} {} page {}/{} zoom {:.3} window {:.3}/{:.3}",
            width,
            height,
            image.sample_label(),
            self.page + 1,
            self.image.pages.len(),
            self.view.zoom,
            self.window.center,
            self.window.width
        )
    }

    pub fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            title: self.title(),
            status: self.status(),
            statistics: self.statistics(),
            histogram: self.histogram(),
        }
    }

    pub fn is_frame_dirty(&self) -> bool {
        self.frame_dirty || self.rgba_dirty || self.fit_pending
    }

    pub fn mark_frame_clean(&mut self) {
        self.frame_dirty = false;
    }

    pub fn render_window(&mut self, width: u32, height: u32) -> Vec<u8> {
        self.ensure_rgba_cache();
        let frame = self.rgba_cache.as_ref().expect("RGBA cache is initialized");
        if self.fit_pending {
            self.view.zoom = fit_zoom(frame.width, frame.height, width, height);
            self.view.center_x = frame.width as f32 / 2.0;
            self.view.center_y = frame.height as f32 / 2.0;
            self.fit_pending = false;
        }

        rasterize_view(frame, width, height, self.view)
    }

    pub fn apply_command(&mut self, command: AppCommand) -> Result<CommandResult> {
        match command {
            AppCommand::Brightness(delta) => {
                self.window.center += delta;
                self.mark_rgba_dirty();
                Ok(CommandResult::Updated(format!("brightness {delta:+.3}")))
            }
            AppCommand::Contrast(factor) => {
                self.window.width = (self.window.width / factor.max(0.01)).max(f32::EPSILON);
                self.mark_rgba_dirty();
                Ok(CommandResult::Updated(format!("contrast x{factor:.3}")))
            }
            AppCommand::Fit => {
                self.fit_pending = true;
                self.frame_dirty = true;
                Ok(CommandResult::Updated("fit".to_owned()))
            }
            AppCommand::Help => Ok(CommandResult::Message(help_text())),
            AppCommand::Invert => {
                self.invert = !self.invert;
                self.mark_rgba_dirty();
                Ok(CommandResult::Updated(format!("invert {}", self.invert)))
            }
            AppCommand::Mean {
                x,
                y,
                width,
                height,
            } => {
                let stats = mean_region(self.current_image(), x, y, width, height)?;
                Ok(CommandResult::Message(format!(
                    "mean x={x} y={y} w={width} h={height}: mean={:.6} min={:.6} max={:.6} n={}",
                    stats.mean, stats.min, stats.max, stats.count
                )))
            }
            AppCommand::Open(path) => {
                let image = load_image(&path)?;
                self.image = image;
                self.page = 0;
                self.reset_window();
                self.transform.reset();
                self.fit_pending = true;
                self.mark_rgba_dirty();
                Ok(CommandResult::Updated(format!("opened {}", path.display())))
            }
            AppCommand::PageNext => {
                self.set_page((self.page + 1).min(self.image.pages.len().saturating_sub(1)));
                Ok(CommandResult::Updated(format!("page {}", self.page + 1)))
            }
            AppCommand::PagePrev => {
                self.set_page(self.page.saturating_sub(1));
                Ok(CommandResult::Updated(format!("page {}", self.page + 1)))
            }
            AppCommand::PageSet(page) => {
                self.set_page(page.saturating_sub(1));
                Ok(CommandResult::Updated(format!("page {}", self.page + 1)))
            }
            AppCommand::Pan { dx, dy } => {
                self.view.center_x += dx / self.view.zoom;
                self.view.center_y += dy / self.view.zoom;
                self.frame_dirty = true;
                Ok(CommandResult::Updated("pan".to_owned()))
            }
            AppCommand::Pixel { x, y } => {
                let value = pixel_value(self.current_image(), x, y)?;
                Ok(CommandResult::Message(format!(
                    "pixel x={x} y={y}: {value}"
                )))
            }
            AppCommand::MoveWindow { .. } | AppCommand::PositionWindow { .. } => Ok(
                CommandResult::Message("window movement is handled by the viewer".to_owned()),
            ),
            AppCommand::PluginInstall(path) => Ok(CommandResult::Message(
                plugins::install_plugin(&path)
                    .map(|message| format!("{message}\n{}", self.plugin_hint()))?,
            )),
            AppCommand::PluginList => Ok(CommandResult::Message(plugins::list_plugins()?)),
            AppCommand::PluginRemove(name) => {
                Ok(CommandResult::Message(plugins::remove_plugin(&name)?))
            }
            AppCommand::Quit => Ok(CommandResult::Quit),
            AppCommand::Reset => {
                self.reset_window();
                self.transform.reset();
                self.fit_pending = true;
                self.mark_rgba_dirty();
                Ok(CommandResult::Updated("reset".to_owned()))
            }
            AppCommand::RotateLeft => {
                self.transform.rotate_left();
                self.fit_pending = true;
                self.mark_rgba_dirty();
                Ok(CommandResult::Updated("rotate left".to_owned()))
            }
            AppCommand::RotateRight => {
                self.transform.rotate_right();
                self.fit_pending = true;
                self.mark_rgba_dirty();
                Ok(CommandResult::Updated("rotate right".to_owned()))
            }
            AppCommand::RunPlugin { name, args } => Ok(CommandResult::Message(
                plugins::run_plugin(&name, &args, &self.image.path)?,
            )),
            AppCommand::SetWindow { center, width } => {
                self.window = Window::new(center, width);
                self.mark_rgba_dirty();
                Ok(CommandResult::Updated(format!(
                    "window {center:.3} {width:.3}"
                )))
            }
            AppCommand::ZoomBy(factor) => {
                self.view.zoom = (self.view.zoom * factor).clamp(0.01, 256.0);
                self.frame_dirty = true;
                Ok(CommandResult::Updated(format!(
                    "zoom {:.3}",
                    self.view.zoom
                )))
            }
            AppCommand::ZoomTo(zoom) => {
                self.view.zoom = zoom.clamp(0.01, 256.0);
                self.frame_dirty = true;
                Ok(CommandResult::Updated(format!(
                    "zoom {:.3}",
                    self.view.zoom
                )))
            }
        }
    }

    fn current_image(&self) -> &ImageData {
        &self.image.pages[self.page].data
    }

    fn ensure_rgba_cache(&mut self) {
        if !self.rgba_dirty && self.rgba_cache.is_some() {
            return;
        }
        let frame = render_to_rgba(
            self.current_image(),
            self.window,
            self.invert,
            self.transform,
        );
        self.rgba_cache = Some(frame);
        if let Some(frame) = &self.rgba_cache {
            self.view.center_x = self
                .view
                .center_x
                .clamp(0.0, frame.width.saturating_sub(1) as f32);
            self.view.center_y = self
                .view
                .center_y
                .clamp(0.0, frame.height.saturating_sub(1) as f32);
        }
        self.rgba_dirty = false;
        self.frame_dirty = true;
    }

    fn mark_rgba_dirty(&mut self) {
        self.rgba_dirty = true;
        self.frame_dirty = true;
    }

    fn reset_window(&mut self) {
        self.window = auto_window(self.current_image(), self.auto_window_mode);
        let (width, height) = self.current_image().dimensions();
        self.view.center_x = width as f32 / 2.0;
        self.view.center_y = height as f32 / 2.0;
    }

    fn set_page(&mut self, page: usize) {
        let page = page.min(self.image.pages.len().saturating_sub(1));
        if self.page != page {
            self.page = page;
            self.reset_window();
            self.fit_pending = true;
            self.mark_rgba_dirty();
        }
    }

    fn plugin_hint(&self) -> String {
        "use `run <plugin> [args...]` to execute installed external-command plugins".to_owned()
    }

    fn statistics(&self) -> String {
        let image = self.current_image();
        let (width, height) = image.dimensions();
        match mean_region(image, 0, 0, width, height) {
            Ok(stats) => format!(
                "area: {}\nmean: {:.6}\nmin: {:.6}\nmax: {:.6}\npage: {}/{}\nzoom: {:.3}\nwindow center: {:.3}\nwindow width: {:.3}\ninvert: {}",
                stats.count,
                stats.mean,
                stats.min,
                stats.max,
                self.page + 1,
                self.image.pages.len(),
                self.view.zoom,
                self.window.center,
                self.window.width,
                self.invert
            ),
            Err(err) => format!("statistics unavailable: {err}"),
        }
    }

    fn histogram(&self) -> String {
        histogram_text(self.current_image(), 16, 18)
    }
}

#[derive(Debug, Clone)]
pub enum CommandResult {
    Message(String),
    Quit,
    Updated(String),
}

#[derive(Debug, Clone, Copy)]
struct RegionStats {
    mean: f64,
    min: f64,
    max: f64,
    count: usize,
}

fn fit_zoom(source_width: u32, source_height: u32, target_width: u32, target_height: u32) -> f32 {
    if source_width == 0 || source_height == 0 || target_width == 0 || target_height == 0 {
        return 1.0;
    }
    (target_width as f32 / source_width as f32)
        .min(target_height as f32 / source_height as f32)
        .clamp(0.01, 256.0)
}

fn pixel_value(image: &ImageData, x: u32, y: u32) -> Result<String> {
    let (width, height) = image.dimensions();
    if x >= width || y >= height {
        return Err(anyhow!("pixel is outside image bounds {width}x{height}"));
    }
    let index = (y as usize * width as usize + x as usize) * samples(image);
    Ok(match image {
        ImageData::Gray8 { data, .. } => data[index].to_string(),
        ImageData::Gray16 { data, .. } => data[index].to_string(),
        ImageData::Gray32F { data, .. } => format!("{:.6}", data[index]),
        ImageData::Rgb8 { data, .. } => {
            format!("{},{},{}", data[index], data[index + 1], data[index + 2])
        }
        ImageData::Rgb16 { data, .. } => {
            format!("{},{},{}", data[index], data[index + 1], data[index + 2])
        }
        ImageData::Rgb32F { data, .. } => {
            format!(
                "{:.6},{:.6},{:.6}",
                data[index],
                data[index + 1],
                data[index + 2]
            )
        }
        ImageData::Rgba8 { data, .. } => {
            format!(
                "{},{},{},{}",
                data[index],
                data[index + 1],
                data[index + 2],
                data[index + 3]
            )
        }
        ImageData::Rgba16 { data, .. } => {
            format!(
                "{},{},{},{}",
                data[index],
                data[index + 1],
                data[index + 2],
                data[index + 3]
            )
        }
        ImageData::Rgba32F { data, .. } => format!(
            "{:.6},{:.6},{:.6},{:.6}",
            data[index],
            data[index + 1],
            data[index + 2],
            data[index + 3]
        ),
    })
}

fn mean_region(image: &ImageData, x: u32, y: u32, width: u32, height: u32) -> Result<RegionStats> {
    let (image_width, image_height) = image.dimensions();
    if x >= image_width || y >= image_height || width == 0 || height == 0 {
        return Err(anyhow!(
            "region is outside image bounds {image_width}x{image_height}"
        ));
    }
    let end_x = x.saturating_add(width).min(image_width);
    let end_y = y.saturating_add(height).min(image_height);
    let mut sum = 0.0;
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut count = 0;

    for yy in y..end_y {
        for xx in x..end_x {
            let value = luminance_at(image, xx, yy);
            if value.is_finite() {
                sum += value;
                min = min.min(value);
                max = max.max(value);
                count += 1;
            }
        }
    }

    if count == 0 {
        return Err(anyhow!("region contains no finite pixels"));
    }

    Ok(RegionStats {
        mean: sum / count as f64,
        min,
        max,
        count,
    })
}

fn luminance_at(image: &ImageData, x: u32, y: u32) -> f64 {
    let (width, _) = image.dimensions();
    let index = (y as usize * width as usize + x as usize) * samples(image);
    match image {
        ImageData::Gray8 { data, .. } => data[index] as f64,
        ImageData::Gray16 { data, .. } => data[index] as f64,
        ImageData::Gray32F { data, .. } => data[index] as f64,
        ImageData::Rgb8 { data, .. } => rgb_luma(
            data[index] as f64,
            data[index + 1] as f64,
            data[index + 2] as f64,
        ),
        ImageData::Rgb16 { data, .. } => rgb_luma(
            data[index] as f64,
            data[index + 1] as f64,
            data[index + 2] as f64,
        ),
        ImageData::Rgb32F { data, .. } => rgb_luma(
            data[index] as f64,
            data[index + 1] as f64,
            data[index + 2] as f64,
        ),
        ImageData::Rgba8 { data, .. } => rgb_luma(
            data[index] as f64,
            data[index + 1] as f64,
            data[index + 2] as f64,
        ),
        ImageData::Rgba16 { data, .. } => rgb_luma(
            data[index] as f64,
            data[index + 1] as f64,
            data[index + 2] as f64,
        ),
        ImageData::Rgba32F { data, .. } => rgb_luma(
            data[index] as f64,
            data[index + 1] as f64,
            data[index + 2] as f64,
        ),
    }
}

fn rgb_luma(r: f64, g: f64, b: f64) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn samples(image: &ImageData) -> usize {
    match image {
        ImageData::Gray8 { .. } | ImageData::Gray16 { .. } | ImageData::Gray32F { .. } => 1,
        ImageData::Rgb8 { .. } | ImageData::Rgb16 { .. } | ImageData::Rgb32F { .. } => 3,
        ImageData::Rgba8 { .. } | ImageData::Rgba16 { .. } | ImageData::Rgba32F { .. } => 4,
    }
}

fn histogram_text(image: &ImageData, bins: usize, bar_width: usize) -> String {
    histogram_channels(image, bins)
        .into_iter()
        .map(|channel| {
            let max = channel.counts.iter().copied().max().unwrap_or(0).max(1);
            let mut lines = vec![format!(
                "{} [{:.3}, {:.3}]",
                channel.name, channel.min, channel.max
            )];
            for (index, count) in channel.counts.iter().enumerate() {
                let width = ((*count as f64 / max as f64) * bar_width as f64).round() as usize;
                lines.push(format!(
                    "{index:02} {:>7} {}",
                    count,
                    "#".repeat(width.max((*count > 0) as usize))
                ));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[derive(Debug)]
struct HistogramChannel {
    name: &'static str,
    min: f64,
    max: f64,
    counts: Vec<usize>,
}

fn histogram_channels(image: &ImageData, bins: usize) -> Vec<HistogramChannel> {
    let mut values = channel_values(image);
    values
        .iter_mut()
        .map(|(name, values)| {
            values.retain(|value| value.is_finite());
            if values.is_empty() {
                return HistogramChannel {
                    name,
                    min: 0.0,
                    max: 0.0,
                    counts: vec![0; bins],
                };
            }
            let min = values.iter().fold(f64::INFINITY, |a, &b| a.min(b));
            let max = values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
            let mut counts = vec![0; bins];
            let span = (max - min).max(f64::EPSILON);
            for value in values {
                let bin = (((*value - min) / span) * (bins - 1) as f64).round() as usize;
                counts[bin.min(bins - 1)] += 1;
            }
            HistogramChannel {
                name,
                min,
                max,
                counts,
            }
        })
        .collect()
}

fn channel_values(image: &ImageData) -> Vec<(&'static str, Vec<f64>)> {
    match image {
        ImageData::Gray8 { data, .. } => vec![("Y", data.iter().map(|&v| v as f64).collect())],
        ImageData::Gray16 { data, .. } => vec![("Y", data.iter().map(|&v| v as f64).collect())],
        ImageData::Gray32F { data, .. } => vec![("Y", data.iter().map(|&v| v as f64).collect())],
        ImageData::Rgb8 { data, .. } => split_channels(data, 3, &["R", "G", "B"], |v| *v as f64),
        ImageData::Rgb16 { data, .. } => split_channels(data, 3, &["R", "G", "B"], |v| *v as f64),
        ImageData::Rgb32F { data, .. } => split_channels(data, 3, &["R", "G", "B"], |v| *v as f64),
        ImageData::Rgba8 { data, .. } => {
            split_channels(data, 4, &["R", "G", "B", "A"], |v| *v as f64)
        }
        ImageData::Rgba16 { data, .. } => {
            split_channels(data, 4, &["R", "G", "B", "A"], |v| *v as f64)
        }
        ImageData::Rgba32F { data, .. } => {
            split_channels(data, 4, &["R", "G", "B", "A"], |v| *v as f64)
        }
    }
}

fn split_channels<T>(
    data: &[T],
    channels: usize,
    names: &'static [&'static str],
    map: impl Fn(&T) -> f64,
) -> Vec<(&'static str, Vec<f64>)> {
    let mut output = names
        .iter()
        .map(|&name| (name, Vec::with_capacity(data.len() / channels)))
        .collect::<Vec<_>>();
    for pixel in data.chunks_exact(channels) {
        for index in 0..channels {
            output[index].1.push(map(&pixel[index]));
        }
    }
    output
}

pub fn help_text() -> String {
    [
        "commands:",
        "  q | quit",
        "  + | - | zoom <value>",
        "  h/j/k/l | pan <dx> <dy>",
        "  [ | ] | page prev|next|<n>",
        "  r | R | rotate right|left",
        "  f | fit",
        "  i | invert",
        "  b <delta> | c <factor>",
        "  window <center> <width>",
        "  pixel <x> <y>",
        "  mean <x> <y> <w> <h>",
        "  open <path>",
        "  plugin install <path>",
        "  plugin list",
        "  plugin remove <name>",
        "  run <plugin> [args...]",
    ]
    .join("\n")
}
