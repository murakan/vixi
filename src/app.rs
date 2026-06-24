// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;
use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, Vec2};

use crate::command::Command;
use crate::pixel::{ImageData, LoadedImage};
use crate::render::{render_to_color_image, DisplayTransform};
use crate::tui::StatusSnapshot;
use crate::windowing::{auto_window, AutoWindowMode, Window};

#[cfg(feature = "video")]
use crate::tui::VideoStatus;
#[cfg(feature = "video")]
use crate::video::{VideoCommand, VideoHandle};

#[derive(Debug, Clone, Copy)]
pub struct ViewerOptions {
    pub window_center: Option<f32>,
    pub window_width: Option<f32>,
    pub auto_window: AutoWindowMode,
    pub invert: bool,
    pub fit: bool,
}

const ZOOM_STEP: f32 = 1.25;
const ZOOM_MIN: f32 = 0.01;
const ZOOM_MAX: f32 = 128.0;

/// The thing being viewed: a still (possibly multipage/animated) image or a
/// streaming video.
pub enum Content {
    Image(ImageContent),
    #[cfg(feature = "video")]
    Video(VideoContent),
}

pub struct ImageContent {
    image: LoadedImage,
    page: usize,
}

#[cfg(feature = "video")]
pub struct VideoContent {
    source: VideoHandle,
    path: PathBuf,
    frame: Option<ImageData>,
    position: f64,
    playing: bool,
    speed: f64,
}

impl Content {
    pub fn image(image: LoadedImage, page: usize) -> Self {
        let page = page.min(image.pages.len().saturating_sub(1));
        Content::Image(ImageContent { image, page })
    }

    #[cfg(feature = "video")]
    pub fn video(source: VideoHandle, path: PathBuf) -> Self {
        Content::Video(VideoContent {
            source,
            path,
            frame: None,
            position: 0.0,
            playing: true,
            speed: 1.0,
        })
    }

    fn path(&self) -> PathBuf {
        match self {
            Content::Image(content) => content.image.path.clone(),
            #[cfg(feature = "video")]
            Content::Video(content) => content.path.clone(),
        }
    }

    fn current_image(&self) -> Option<&ImageData> {
        match self {
            Content::Image(content) => Some(&content.image.pages[content.page].data),
            #[cfg(feature = "video")]
            Content::Video(content) => content.frame.as_ref(),
        }
    }
}

pub struct ViewerApp {
    content: Content,
    path: PathBuf,
    window: Window,
    auto_window_mode: AutoWindowMode,
    invert: bool,
    fit: bool,
    zoom: f32,
    pan: Vec2,
    transform: DisplayTransform,
    texture: Option<TextureHandle>,
    texture_dirty: bool,
    commands: Receiver<Command>,
    status: Sender<StatusSnapshot>,
}

impl ViewerApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        content: Content,
        options: ViewerOptions,
        commands: Receiver<Command>,
        status: Sender<StatusSnapshot>,
    ) -> Self {
        let path = content.path();
        let window = match content.current_image() {
            Some(image) => {
                let auto = auto_window(image, options.auto_window);
                Window::new(
                    options.window_center.unwrap_or(auto.center),
                    options.window_width.unwrap_or(auto.width),
                )
            }
            None => Window::new(0.5, 1.0),
        };

        Self {
            content,
            path,
            window,
            auto_window_mode: options.auto_window,
            invert: options.invert,
            fit: options.fit,
            zoom: 1.0,
            pan: Vec2::ZERO,
            transform: DisplayTransform::default(),
            texture: None,
            texture_dirty: true,
            commands,
            status,
        }
    }

    fn reset_window(&mut self) {
        if let Some(image) = self.content.current_image() {
            self.window = auto_window(image, self.auto_window_mode);
            self.texture_dirty = true;
        }
    }

    fn set_page(&mut self, page: usize) {
        // `Content` has a single variant when built without the `video`
        // feature, which makes this `if let` irrefutable in that configuration.
        #[cfg_attr(not(feature = "video"), allow(irrefutable_let_patterns))]
        let changed = if let Content::Image(content) = &mut self.content {
            let page = page.min(content.image.pages.len().saturating_sub(1));
            if content.page != page {
                content.page = page;
                true
            } else {
                false
            }
        } else {
            false
        };
        if changed {
            self.reset_window();
        }
    }

    fn next_page(&mut self) {
        #[cfg(feature = "video")]
        if let Content::Video(content) = &mut self.content {
            content.playing = false;
            content.source.send(VideoCommand::Step);
            return;
        }
        let target = match &self.content {
            Content::Image(content) => content.page + 1,
            #[cfg(feature = "video")]
            Content::Video(_) => return,
        };
        self.set_page(target);
    }

    fn prev_page(&mut self) {
        #[cfg(feature = "video")]
        if let Content::Video(content) = &mut self.content {
            let step = if content.source.info.fps > 0.0 {
                1.0 / content.source.info.fps
            } else {
                0.04
            };
            content.source.send(VideoCommand::SeekRelative(-step));
            return;
        }
        let target = match &self.content {
            Content::Image(content) => content.page.saturating_sub(1),
            #[cfg(feature = "video")]
            Content::Video(_) => return,
        };
        self.set_page(target);
    }

    fn apply(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            // Handled by the REPL, never reaches the GUI.
            Command::Help | Command::Status => {}
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::NextPage => self.next_page(),
            Command::PrevPage => self.prev_page(),
            Command::GotoPage(page) => self.set_page(page),
            Command::ZoomIn => self.set_zoom(self.zoom * ZOOM_STEP),
            Command::ZoomOut => self.set_zoom(self.zoom / ZOOM_STEP),
            Command::ZoomSet(factor) => self.set_zoom(factor),
            Command::Fit => self.fit = true,
            Command::Pan(dx, dy) => self.pan += Vec2::new(dx, dy),
            Command::PanReset => self.pan = Vec2::ZERO,
            Command::ResetWindow => self.reset_window(),
            Command::SetWindowCenter(center) => {
                self.window.center = center;
                self.texture_dirty = true;
            }
            Command::SetWindowWidth(width) => {
                self.window.width = width.max(f32::EPSILON);
                self.texture_dirty = true;
            }
            Command::SetWindow { center, width } => {
                self.window = Window::new(center, width);
                self.texture_dirty = true;
            }
            Command::AutoWindow(mode) => {
                self.auto_window_mode = mode;
                self.reset_window();
            }
            Command::Invert => {
                self.invert = !self.invert;
                self.texture_dirty = true;
            }
            Command::RotateLeft => {
                self.transform.rotate_left();
                self.texture_dirty = true;
                self.fit = true;
            }
            Command::RotateRight => {
                self.transform.rotate_right();
                self.texture_dirty = true;
                self.fit = true;
            }
            Command::FlipX => {
                self.transform.toggle_flip_x();
                self.texture_dirty = true;
            }
            Command::FlipY => {
                self.transform.toggle_flip_y();
                self.texture_dirty = true;
            }
            Command::OrientReset => {
                self.transform.reset();
                self.texture_dirty = true;
                self.fit = true;
            }
            Command::Play
            | Command::Pause
            | Command::TogglePlay
            | Command::Seek(_)
            | Command::SeekRelative(_)
            | Command::Speed(_)
            | Command::Step => self.video_command(command),
        }
    }

    #[cfg(feature = "video")]
    fn video_command(&mut self, command: Command) {
        let Content::Video(content) = &mut self.content else {
            return;
        };
        let video_command = match command {
            Command::Play => {
                content.playing = true;
                VideoCommand::Play
            }
            Command::Pause => {
                content.playing = false;
                VideoCommand::Pause
            }
            Command::TogglePlay => {
                content.playing = !content.playing;
                VideoCommand::TogglePlay
            }
            Command::Step => {
                content.playing = false;
                VideoCommand::Step
            }
            Command::Seek(secs) => VideoCommand::Seek(secs),
            Command::SeekRelative(secs) => VideoCommand::SeekRelative(secs),
            Command::Speed(speed) => {
                content.speed = speed;
                VideoCommand::SetSpeed(speed)
            }
            _ => return,
        };
        content.source.send(video_command);
    }

    #[cfg(not(feature = "video"))]
    fn video_command(&mut self, _command: Command) {}

    fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    }

    /// Pull the latest decoded video frame, if any. Returns true if updated.
    #[cfg(feature = "video")]
    fn poll_video(&mut self) -> bool {
        if let Content::Video(content) = &mut self.content {
            if let Some(frame) = content.source.latest_frame() {
                content.frame = Some(frame.data);
                content.position = frame.pts_secs;
                self.texture_dirty = true;
                return true;
            }
        }
        false
    }

    #[cfg(not(feature = "video"))]
    fn poll_video(&mut self) -> bool {
        false
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        if !self.texture_dirty {
            return;
        }
        let Some(image) = self.content.current_image() else {
            return;
        };

        let color_image: ColorImage =
            render_to_color_image(image, self.window, self.invert, self.transform);
        if let Some(texture) = &mut self.texture {
            texture.set(color_image, TextureOptions::NEAREST);
        } else {
            self.texture = Some(ctx.load_texture("image", color_image, TextureOptions::NEAREST));
        }
        self.texture_dirty = false;
    }

    fn snapshot(&self) -> StatusSnapshot {
        match &self.content {
            Content::Image(content) => {
                let image = &content.image.pages[content.page].data;
                let (width, height) = image.dimensions();
                StatusSnapshot {
                    path: self.path.display().to_string(),
                    page: content.page,
                    page_count: content.image.pages.len(),
                    delay_ms: content.image.pages[content.page].delay_ms,
                    width,
                    height,
                    sample: image.sample_label().to_owned(),
                    zoom: self.zoom,
                    pan: (self.pan.x, self.pan.y),
                    windowable: image.is_windowable(),
                    window_center: self.window.center,
                    window_width: self.window.width,
                    invert: self.invert,
                    rotation_degrees: self.transform.rotation_degrees(),
                    flip_x: self.transform.flip_x(),
                    flip_y: self.transform.flip_y(),
                    auto_window: auto_window_label(self.auto_window_mode).to_owned(),
                    video: None,
                }
            }
            #[cfg(feature = "video")]
            Content::Video(content) => {
                let (width, height) = content
                    .frame
                    .as_ref()
                    .map(ImageData::dimensions)
                    .unwrap_or((content.source.info.width, content.source.info.height));
                StatusSnapshot {
                    path: self.path.display().to_string(),
                    page: 0,
                    page_count: 1,
                    delay_ms: None,
                    width,
                    height,
                    sample: "RGBA8 (video)".to_owned(),
                    zoom: self.zoom,
                    pan: (self.pan.x, self.pan.y),
                    windowable: false,
                    window_center: 0.0,
                    window_width: 0.0,
                    invert: self.invert,
                    rotation_degrees: self.transform.rotation_degrees(),
                    flip_x: self.transform.flip_x(),
                    flip_y: self.transform.flip_y(),
                    auto_window: "-".to_owned(),
                    video: Some(VideoStatus {
                        playing: content.playing,
                        speed: content.speed,
                        position_secs: content.position,
                        duration_secs: content.source.info.duration_secs,
                        fps: content.source.info.fps,
                    }),
                }
            }
        }
    }
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_video();

        let mut processed = false;
        while let Ok(command) = self.commands.try_recv() {
            self.apply(command, ctx);
            processed = true;
        }

        self.ensure_texture(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::BLACK))
            .show(ctx, |ui| {
                let Some(texture) = &self.texture else {
                    return;
                };
                let [width, height] = texture.size();
                let available = ui.available_rect_before_wrap();

                if self.fit {
                    let scale_x = available.width() / width as f32;
                    let scale_y = available.height() / height as f32;
                    self.zoom = scale_x.min(scale_y).clamp(ZOOM_MIN, ZOOM_MAX);
                    self.pan = Vec2::ZERO;
                    self.fit = false;
                }

                let size = Vec2::new(width as f32 * self.zoom, height as f32 * self.zoom);
                let rect = Rect::from_center_size(available.center() + self.pan, size);
                egui::Image::new((texture.id(), size)).paint_at(ui, rect);
            });

        // Report the resulting state back to the REPL after the frame is laid
        // out so a fit-driven zoom is reflected in the snapshot.
        if processed {
            let _ = self.status.send(self.snapshot());
        }
    }
}

fn auto_window_label(mode: AutoWindowMode) -> &'static str {
    match mode {
        AutoWindowMode::MinMax => "minmax",
        AutoWindowMode::Percentile { .. } => "percentile",
    }
}
