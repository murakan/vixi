// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use crate::pixel::ImageData;
use crate::windowing::{map_to_u8, Window};

#[derive(Debug, Clone)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

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

    pub fn reset(&mut self) {
        *self = Self::default();
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
        match self.quarter_turns {
            0 => (x, y),
            1 => (y, source_height - 1 - x),
            2 => (source_width - 1 - x, source_height - 1 - y),
            3 => (source_width - 1 - y, x),
            _ => unreachable!(),
        }
    }
}

pub fn render_to_rgba(
    image: &ImageData,
    window: Window,
    invert: bool,
    transform: DisplayTransform,
) -> RgbaFrame {
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

    RgbaFrame {
        width: display_width,
        height: display_height,
        data: rgba,
    }
}

pub fn rasterize_view(
    source: &RgbaFrame,
    target_width: u32,
    target_height: u32,
    view: View,
) -> Vec<u8> {
    let mut target = vec![20; target_width as usize * target_height as usize * 4];
    for px in target.chunks_exact_mut(4) {
        px[3] = 255;
    }

    if source.width == 0 || source.height == 0 || target_width == 0 || target_height == 0 {
        return target;
    }

    for y in 0..target_height {
        for x in 0..target_width {
            let sx = view.center_x + (x as f32 + 0.5 - target_width as f32 / 2.0) / view.zoom;
            let sy = view.center_y + (y as f32 + 0.5 - target_height as f32 / 2.0) / view.zoom;
            if sx < 0.0 || sy < 0.0 || sx >= source.width as f32 || sy >= source.height as f32 {
                continue;
            }

            let sx = sx.floor() as u32;
            let sy = sy.floor() as u32;
            let src = (sy as usize * source.width as usize + sx as usize) * 4;
            let dst = (y as usize * target_width as usize + x as usize) * 4;
            target[dst..dst + 4].copy_from_slice(&source.data[src..src + 4]);
        }
    }

    target
}

#[derive(Debug, Clone, Copy)]
pub struct View {
    pub center_x: f32,
    pub center_y: f32,
    pub zoom: f32,
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
    fn rotates_display_rgba() {
        let source = [
            1, 1, 1, 255, 2, 2, 2, 255, 3, 3, 3, 255, 4, 4, 4, 255, 5, 5, 5, 255, 6, 6, 6, 255,
        ];
        let mut transform = DisplayTransform::default();
        transform.rotate_right();

        let target = transform_rgba(&source, 2, 3, transform);
        let values: Vec<u8> = target.chunks_exact(4).map(|px| px[0]).collect();
        assert_eq!(values, vec![5, 3, 1, 6, 4, 2]);
    }
}
