// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use eframe::egui;
use egui::{ColorImage, TextureHandle, TextureOptions, Vec2};

use crate::pixel::LoadedImage;
use crate::render::{render_to_color_image, DisplayTransform};
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

pub struct ViewerApp {
    image: LoadedImage,
    page: usize,
    window: Window,
    auto_window_mode: AutoWindowMode,
    invert: bool,
    fit: bool,
    zoom: f32,
    transform: DisplayTransform,
    texture: Option<TextureHandle>,
    texture_dirty: bool,
}

impl ViewerApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        image: LoadedImage,
        options: ViewerOptions,
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
            transform: DisplayTransform::default(),
            texture: None,
            texture_dirty: true,
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

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input(|input| {
            if input.key_pressed(egui::Key::R) {
                self.reset_window();
            }
            if input.key_pressed(egui::Key::I) {
                self.invert = !self.invert;
                self.texture_dirty = true;
            }
            if input.key_pressed(egui::Key::F) {
                self.fit = true;
            }
            if input.key_pressed(egui::Key::Q) {
                self.transform.rotate_left();
                self.texture_dirty = true;
                self.fit = true;
            }
            if input.key_pressed(egui::Key::E) {
                self.transform.rotate_right();
                self.texture_dirty = true;
                self.fit = true;
            }
            if input.key_pressed(egui::Key::H) {
                self.transform.toggle_flip_x();
                self.texture_dirty = true;
            }
            if input.key_pressed(egui::Key::V) {
                self.transform.toggle_flip_y();
                self.texture_dirty = true;
            }
            if input.key_pressed(egui::Key::Num0) {
                self.transform.reset();
                self.texture_dirty = true;
                self.fit = true;
            }
            if input.key_pressed(egui::Key::ArrowRight)
                || input.key_pressed(egui::Key::CloseBracket)
            {
                self.set_page(self.page + 1);
            }
            if (input.key_pressed(egui::Key::ArrowLeft)
                || input.key_pressed(egui::Key::OpenBracket))
                && self.page > 0
            {
                self.set_page(self.page - 1);
            }
        });
    }

    fn apply_window_drag(&mut self, delta: Vec2) {
        if !self.current_image().is_windowable() || delta == Vec2::ZERO {
            return;
        }

        let speed = self.window.width.abs().max(1.0) / 300.0;
        self.window.width = (self.window.width + delta.x * speed).max(f32::EPSILON);
        self.window.center -= delta.y * speed;
        self.texture_dirty = true;
    }
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_shortcuts(ctx);
        self.ensure_texture(ctx);

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.image.path.display().to_string());
                ui.separator();
                let (width, height) = self.current_image().dimensions();
                ui.label(format!(
                    "{}x{} {}",
                    width,
                    height,
                    self.current_image().sample_label()
                ));
                if self.image.pages.len() > 1 {
                    ui.separator();
                    ui.label("Page");
                    let mut page = self.page;
                    if ui
                        .add(egui::DragValue::new(&mut page).range(0..=self.image.pages.len() - 1))
                        .changed()
                    {
                        self.set_page(page);
                    }
                    ui.label(format!("/ {}", self.image.pages.len() - 1));
                }
                ui.separator();
                if ui.button("Fit").clicked() {
                    self.fit = true;
                }
                if ui.button("Reset").clicked() {
                    self.reset_window();
                }
                if ui.checkbox(&mut self.invert, "Invert").changed() {
                    self.texture_dirty = true;
                }
                ui.separator();
                if ui.button("Rot L").clicked() {
                    self.transform.rotate_left();
                    self.texture_dirty = true;
                    self.fit = true;
                }
                if ui.button("Rot R").clicked() {
                    self.transform.rotate_right();
                    self.texture_dirty = true;
                    self.fit = true;
                }
                if ui.button("Flip X").clicked() {
                    self.transform.toggle_flip_x();
                    self.texture_dirty = true;
                }
                if ui.button("Flip Y").clicked() {
                    self.transform.toggle_flip_y();
                    self.texture_dirty = true;
                }
                if ui.button("Orient 0").clicked() {
                    self.transform.reset();
                    self.texture_dirty = true;
                    self.fit = true;
                }
            });
            if self.current_image().is_windowable() {
                ui.horizontal(|ui| {
                    let drag_speed = self.window.width.abs().max(1.0) / 200.0;
                    let center_changed = ui
                        .add(egui::DragValue::new(&mut self.window.center).speed(drag_speed))
                        .changed();
                    ui.label("Center");
                    let width_changed = ui
                        .add(egui::DragValue::new(&mut self.window.width).speed(drag_speed))
                        .changed();
                    ui.label("Width");
                    if center_changed || width_changed {
                        self.window.width = self.window.width.max(f32::EPSILON);
                        self.texture_dirty = true;
                    }
                });
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(texture) = &self.texture else {
                return;
            };
            let texture_id = texture.id();
            let [width, height] = texture.size();
            let available = ui.available_size();
            if self.fit {
                let scale_x = available.x / width as f32;
                let scale_y = available.y / height as f32;
                self.zoom = scale_x.min(scale_y).max(0.01);
                self.fit = false;
            }

            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll != 0.0 && ui.rect_contains_pointer(ui.max_rect()) {
                let factor = (1.0_f32 + scroll / 600.0).clamp(0.2, 5.0);
                self.zoom = (self.zoom * factor).clamp(0.01, 128.0);
            }

            let size = Vec2::new(width as f32 * self.zoom, height as f32 * self.zoom);
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let response = ui.add(
                        egui::Image::new((texture_id, size)).sense(egui::Sense::click_and_drag()),
                    );
                    if response.dragged_by(egui::PointerButton::Secondary) {
                        let delta = ui.input(|input| input.pointer.delta());
                        self.apply_window_drag(delta);
                    }
                });
        });
    }
}
