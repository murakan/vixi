// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;
use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, Vec2};

use crate::command::Command;
use crate::pixel::LoadedImage;
use crate::render::{render_to_color_image, DisplayTransform};
use crate::tui::StatusSnapshot;
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

const ZOOM_STEP: f32 = 1.25;
const ZOOM_MIN: f32 = 0.01;
const ZOOM_MAX: f32 = 128.0;

pub struct ViewerApp {
    image: LoadedImage,
    page: usize,
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
        image: LoadedImage,
        options: ViewerOptions,
        commands: Receiver<Command>,
        status: Sender<StatusSnapshot>,
    ) -> Self {
        let page = options.page.min(image.pages.len().saturating_sub(1));
        let auto = auto_window(&image.pages[page].data, options.auto_window);
        let window = Window::new(
            options.window_center.unwrap_or(auto.center),
            options.window_width.unwrap_or(auto.width),
        );

        Self {
            image,
            page,
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

    fn current_image(&self) -> &crate::pixel::ImageData {
        &self.image.pages[self.page].data
    }

    fn reset_window(&mut self) {
        self.window = auto_window(self.current_image(), self.auto_window_mode);
        self.texture_dirty = true;
    }

    fn set_page(&mut self, page: usize) {
        let page = page.min(self.image.pages.len().saturating_sub(1));
        if self.page != page {
            self.page = page;
            self.reset_window();
        }
    }

    fn apply(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            // Handled by the REPL, never reaches the GUI.
            Command::Help | Command::Status => {}
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::NextPage => self.set_page(self.page + 1),
            Command::PrevPage => self.set_page(self.page.saturating_sub(1)),
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
        }
    }

    fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        if !self.texture_dirty && self.texture.is_some() {
            return;
        }

        let color_image: ColorImage = render_to_color_image(
            self.current_image(),
            self.window,
            self.invert,
            self.transform,
        );
        if let Some(texture) = &mut self.texture {
            texture.set(color_image, TextureOptions::NEAREST);
        } else {
            self.texture = Some(ctx.load_texture("image", color_image, TextureOptions::NEAREST));
        }
        self.texture_dirty = false;
    }

    fn snapshot(&self) -> StatusSnapshot {
        let (width, height) = self.current_image().dimensions();
        StatusSnapshot {
            path: self.image.path.display().to_string(),
            page: self.page,
            page_count: self.image.pages.len(),
            delay_ms: self.image.pages[self.page].delay_ms,
            width,
            height,
            sample: self.current_image().sample_label().to_owned(),
            zoom: self.zoom,
            pan: (self.pan.x, self.pan.y),
            windowable: self.current_image().is_windowable(),
            window_center: self.window.center,
            window_width: self.window.width,
            invert: self.invert,
            rotation_degrees: self.transform.rotation_degrees(),
            flip_x: self.transform.flip_x(),
            flip_y: self.transform.flip_y(),
            auto_window: auto_window_label(self.auto_window_mode).to_owned(),
        }
    }
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
