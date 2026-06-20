// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use crate::pixel::ImageData;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AutoWindowMode {
    MinMax,
    Percentile { low: f32, high: f32 },
}

#[derive(Debug, Clone, Copy)]
pub struct Window {
    pub center: f32,
    pub width: f32,
}

impl Window {
    pub fn new(center: f32, width: f32) -> Self {
        Self {
            center,
            width: width.max(f32::EPSILON),
        }
    }

    pub fn low_high(self) -> (f32, f32) {
        let half = self.width / 2.0;
        (self.center - half, self.center + half)
    }
}

pub fn auto_window(image: &ImageData, mode: AutoWindowMode) -> Window {
    let mut values = Vec::new();
    collect_luminance(image, &mut values);
    values.retain(|value| value.is_finite());

    if values.is_empty() {
        return Window::new(0.5, 1.0);
    }

    match mode {
        AutoWindowMode::MinMax => {
            let mut min = f32::INFINITY;
            let mut max = f32::NEG_INFINITY;
            for value in values {
                min = min.min(value);
                max = max.max(value);
            }
            Window::new((min + max) / 2.0, (max - min).max(1.0))
        }
        AutoWindowMode::Percentile { low, high } => {
            values.sort_by(|a, b| a.total_cmp(b));
            let low_value = percentile_sorted(&values, low);
            let high_value = percentile_sorted(&values, high);
            Window::new(
                (low_value + high_value) / 2.0,
                (high_value - low_value).max(1.0),
            )
        }
    }
}

pub fn map_to_u8(value: f32, window: Window, invert: bool) -> u8 {
    if value.is_nan() {
        return if invert { 255 } else { 0 };
    }

    let (low, high) = window.low_high();
    let normalized = if value <= low {
        0.0
    } else if value >= high {
        1.0
    } else {
        (value - low) / (high - low)
    };
    let normalized = if invert { 1.0 - normalized } else { normalized };
    (normalized * 255.0).round().clamp(0.0, 255.0) as u8
}

fn collect_luminance(image: &ImageData, values: &mut Vec<f32>) {
    match image {
        ImageData::Gray8 { data, .. } => values.extend(data.iter().map(|&v| v as f32)),
        ImageData::Gray16 { data, .. } => values.extend(data.iter().map(|&v| v as f32)),
        ImageData::Gray32F { data, .. } => values.extend(data.iter().copied()),
        ImageData::Rgb8 { data, .. } => collect_rgb(data, values, |v| v as f32),
        ImageData::Rgb16 { data, .. } => collect_rgb(data, values, |v| v as f32),
        ImageData::Rgb32F { data, .. } => collect_rgb(data, values, |v| v),
        ImageData::Rgba8 { data, .. } => collect_rgba(data, values, |v| v as f32),
        ImageData::Rgba16 { data, .. } => collect_rgba(data, values, |v| v as f32),
        ImageData::Rgba32F { data, .. } => collect_rgba(data, values, |v| v),
    }
}

fn collect_rgb<T: Copy>(data: &[T], values: &mut Vec<f32>, map: impl Fn(T) -> f32) {
    values.extend(data.chunks_exact(3).map(|px| {
        let r = map(px[0]);
        let g = map(px[1]);
        let b = map(px[2]);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }));
}

fn collect_rgba<T: Copy>(data: &[T], values: &mut Vec<f32>, map: impl Fn(T) -> f32) {
    values.extend(data.chunks_exact(4).map(|px| {
        let r = map(px[0]);
        let g = map(px[1]);
        let b = map(px[2]);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }));
}

fn percentile_sorted(values: &[f32], percentile: f32) -> f32 {
    let percentile = percentile.clamp(0.0, 100.0);
    let index = ((values.len() - 1) as f32 * percentile / 100.0).round() as usize;
    values[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_window_edges() {
        let window = Window::new(50.0, 100.0);
        assert_eq!(map_to_u8(0.0, window, false), 0);
        assert_eq!(map_to_u8(50.0, window, false), 128);
        assert_eq!(map_to_u8(100.0, window, false), 255);
    }

    #[test]
    fn ignores_nan_for_auto_window() {
        let image = ImageData::Gray32F {
            width: 3,
            height: 1,
            data: vec![0.0, f32::NAN, 10.0],
        };
        let window = auto_window(&image, AutoWindowMode::MinMax);
        assert_eq!(window.center, 5.0);
        assert_eq!(window.width, 10.0);
    }
}
