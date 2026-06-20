// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use egui::ColorImage;

use crate::pixel::ImageData;
use crate::windowing::{map_to_u8, Window};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DisplayTransform {
    quarter_turns: u8,
    flip_x: bool,
    flip_y: bool,
}

impl DisplayTransform {
    pub fn rotate_left(&mut self) {
        self.quarter_turns = (self.quarter_turns + 3) % 4;
    }

    pub fn rotate_right(&mut self) {
        self.quarter_turns = (self.quarter_turns + 1) % 4;
    }

    pub fn toggle_flip_x(&mut self) {
        self.flip_x = !self.flip_x;
    }

    pub fn toggle_flip_y(&mut self) {
        self.flip_y = !self.flip_y;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn rotation_degrees(self) -> u32 {
        self.quarter_turns as u32 * 90
    }

    pub fn flip_x(self) -> bool {
        self.flip_x
    }

    pub fn flip_y(self) -> bool {
        self.flip_y
    }

    fn is_identity(self) -> bool {
        self == Self::default()
    }

    fn display_dimensions(self, width: u32, height: u32) -> (u32, u32) {
        if self.quarter_turns % 2 == 0 {
            (width, height)
        } else {
            (height, width)
        }
    }

    fn source_xy(self, x: u32, y: u32, source_width: u32, source_height: u32) -> (u32, u32) {
        let (display_width, display_height) = self.display_dimensions(source_width, source_height);
        let x = if self.flip_x {
            display_width - 1 - x
        } else {
            x
        };
        let y = if self.flip_y {
            display_height - 1 - y
        } else {
            y
        };

        match self.quarter_turns {
            0 => (x, y),
            1 => (y, source_height - 1 - x),
            2 => (source_width - 1 - x, source_height - 1 - y),
            3 => (source_width - 1 - y, x),
            _ => unreachable!(),
        }
    }
}

pub fn render_to_color_image(
    image: &ImageData,
    window: Window,
    invert: bool,
    transform: DisplayTransform,
) -> ColorImage {
    let (width, height) = image.dimensions();
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);

    match image {
        ImageData::Gray8 { data, .. } => {
            for &value in data {
                let value = if invert { 255 - value } else { value };
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        ImageData::Gray16 { data, .. } => {
            push_gray(data.iter().map(|&v| v as f32), window, invert, &mut rgba)
        }
        ImageData::Gray32F { data, .. } => {
            push_gray(data.iter().copied(), window, invert, &mut rgba)
        }
        ImageData::Rgb8 { data, .. } => {
            for px in data.chunks_exact(3) {
                if invert {
                    rgba.extend_from_slice(&[255 - px[0], 255 - px[1], 255 - px[2], 255]);
                } else {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
        }
        ImageData::Rgb16 { data, .. } => {
            push_rgb(data.iter().map(|&v| v as f32), window, invert, &mut rgba)
        }
        ImageData::Rgb32F { data, .. } => push_rgb(data.iter().copied(), window, invert, &mut rgba),
        ImageData::Rgba8 { data, .. } => {
            for px in data.chunks_exact(4) {
                if invert {
                    rgba.extend_from_slice(&[255 - px[0], 255 - px[1], 255 - px[2], px[3]]);
                } else {
                    rgba.extend_from_slice(px);
                }
            }
        }
        ImageData::Rgba16 { data, .. } => push_rgba(
            data.iter().map(|&v| v as f32),
            window,
            Window::new(32767.5, 65535.0),
            invert,
            &mut rgba,
        ),
        ImageData::Rgba32F { data, .. } => push_rgba(
            data.iter().copied(),
            window,
            Window::new(0.5, 1.0),
            invert,
            &mut rgba,
        ),
    }

    let (display_width, display_height) = transform.display_dimensions(width, height);
    if !transform.is_identity() {
        rgba = transform_rgba(&rgba, width, height, transform);
    }

    ColorImage::from_rgba_unmultiplied([display_width as usize, display_height as usize], &rgba)
}

fn transform_rgba(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    transform: DisplayTransform,
) -> Vec<u8> {
    let (display_width, display_height) = transform.display_dimensions(source_width, source_height);
    let mut target = vec![0; display_width as usize * display_height as usize * 4];

    for y in 0..display_height {
        for x in 0..display_width {
            let (source_x, source_y) = transform.source_xy(x, y, source_width, source_height);
            let source_index = (source_y as usize * source_width as usize + source_x as usize) * 4;
            let target_index = (y as usize * display_width as usize + x as usize) * 4;
            target[target_index..target_index + 4]
                .copy_from_slice(&source[source_index..source_index + 4]);
        }
    }

    target
}

fn push_gray(values: impl Iterator<Item = f32>, window: Window, invert: bool, rgba: &mut Vec<u8>) {
    for value in values {
        let value = map_to_u8(value, window, invert);
        rgba.extend_from_slice(&[value, value, value, 255]);
    }
}

fn push_rgb(values: impl Iterator<Item = f32>, window: Window, invert: bool, rgba: &mut Vec<u8>) {
    let mut values = values.peekable();
    while values.peek().is_some() {
        let r = map_to_u8(values.next().unwrap_or(0.0), window, invert);
        let g = map_to_u8(values.next().unwrap_or(0.0), window, invert);
        let b = map_to_u8(values.next().unwrap_or(0.0), window, invert);
        rgba.extend_from_slice(&[r, g, b, 255]);
    }
}

fn push_rgba(
    values: impl Iterator<Item = f32>,
    window: Window,
    alpha_window: Window,
    invert: bool,
    rgba: &mut Vec<u8>,
) {
    let mut values = values.peekable();
    while values.peek().is_some() {
        let r = map_to_u8(values.next().unwrap_or(0.0), window, invert);
        let g = map_to_u8(values.next().unwrap_or(0.0), window, invert);
        let b = map_to_u8(values.next().unwrap_or(0.0), window, invert);
        let a = map_to_u8(values.next().unwrap_or(0.0), alpha_window, false);
        rgba.extend_from_slice(&[r, g, b, a]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_and_flips_display_rgba() {
        let source = [
            1, 1, 1, 255, 2, 2, 2, 255, 3, 3, 3, 255, 4, 4, 4, 255, 5, 5, 5, 255, 6, 6, 6, 255,
        ];
        let mut transform = DisplayTransform::default();
        transform.rotate_right();
        transform.toggle_flip_x();

        let target = transform_rgba(&source, 2, 3, transform);
        let values: Vec<u8> = target.chunks_exact(4).map(|px| px[0]).collect();
        assert_eq!(values, vec![1, 3, 5, 2, 4, 6]);
    }
}
