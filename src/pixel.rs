// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct LoadedImage {
    pub path: PathBuf,
    pub pages: Vec<ImagePage>,
}

#[derive(Debug, Clone)]
pub struct ImagePage {
    pub data: ImageData,
    /// Frame display duration in milliseconds for animated formats
    /// (GIF, APNG, animated WebP). `None` for still pages such as
    /// multipage TIFF.
    pub delay_ms: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum ImageData {
    Gray8 {
        width: u32,
        height: u32,
        data: Vec<u8>,
    },
    Gray16 {
        width: u32,
        height: u32,
        data: Vec<u16>,
    },
    Gray32F {
        width: u32,
        height: u32,
        data: Vec<f32>,
    },
    Rgb8 {
        width: u32,
        height: u32,
        data: Vec<u8>,
    },
    Rgb16 {
        width: u32,
        height: u32,
        data: Vec<u16>,
    },
    Rgb32F {
        width: u32,
        height: u32,
        data: Vec<f32>,
    },
    Rgba8 {
        width: u32,
        height: u32,
        data: Vec<u8>,
    },
    Rgba16 {
        width: u32,
        height: u32,
        data: Vec<u16>,
    },
    Rgba32F {
        width: u32,
        height: u32,
        data: Vec<f32>,
    },
}

impl ImageData {
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Gray8 { width, height, .. }
            | Self::Gray16 { width, height, .. }
            | Self::Gray32F { width, height, .. }
            | Self::Rgb8 { width, height, .. }
            | Self::Rgb16 { width, height, .. }
            | Self::Rgb32F { width, height, .. }
            | Self::Rgba8 { width, height, .. }
            | Self::Rgba16 { width, height, .. }
            | Self::Rgba32F { width, height, .. } => (*width, *height),
        }
    }

    pub fn sample_label(&self) -> &'static str {
        match self {
            Self::Gray8 { .. } => "Gray8",
            Self::Gray16 { .. } => "Gray16",
            Self::Gray32F { .. } => "Gray32F",
            Self::Rgb8 { .. } => "RGB8",
            Self::Rgb16 { .. } => "RGB16",
            Self::Rgb32F { .. } => "RGB32F",
            Self::Rgba8 { .. } => "RGBA8",
            Self::Rgba16 { .. } => "RGBA16",
            Self::Rgba32F { .. } => "RGBA32F",
        }
    }

    pub fn is_windowable(&self) -> bool {
        !matches!(
            self,
            Self::Gray8 { .. } | Self::Rgb8 { .. } | Self::Rgba8 { .. }
        )
    }
}
