// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use image::DynamicImage;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::ColorType;

use crate::pixel::{ImageData, ImagePage, LoadedImage};

pub fn load_image(path: &Path) -> Result<LoadedImage> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if matches!(extension.as_str(), "tif" | "tiff") {
        load_tiff(path)
    } else {
        load_dynamic(path)
    }
}

fn load_dynamic(path: &Path) -> Result<LoadedImage> {
    let image = image::open(path).with_context(|| format!("failed to read {}", path.display()))?;
    let data = from_dynamic(image)?;
    Ok(LoadedImage {
        path: PathBuf::from(path),
        pages: vec![ImagePage { data }],
    })
}

fn from_dynamic(image: DynamicImage) -> Result<ImageData> {
    Ok(match image {
        DynamicImage::ImageLuma8(buffer) => ImageData::Gray8 {
            width: buffer.width(),
            height: buffer.height(),
            data: buffer.into_raw(),
        },
        DynamicImage::ImageLuma16(buffer) => ImageData::Gray16 {
            width: buffer.width(),
            height: buffer.height(),
            data: buffer.into_raw(),
        },
        DynamicImage::ImageRgb8(buffer) => ImageData::Rgb8 {
            width: buffer.width(),
            height: buffer.height(),
            data: buffer.into_raw(),
        },
        DynamicImage::ImageRgb16(buffer) => ImageData::Rgb16 {
            width: buffer.width(),
            height: buffer.height(),
            data: buffer.into_raw(),
        },
        DynamicImage::ImageRgba8(buffer) => ImageData::Rgba8 {
            width: buffer.width(),
            height: buffer.height(),
            data: buffer.into_raw(),
        },
        DynamicImage::ImageRgba16(buffer) => ImageData::Rgba16 {
            width: buffer.width(),
            height: buffer.height(),
            data: buffer.into_raw(),
        },
        other => {
            let rgba = other.to_rgba8();
            ImageData::Rgba8 {
                width: rgba.width(),
                height: rgba.height(),
                data: rgba.into_raw(),
            }
        }
    })
}

fn load_tiff(path: &Path) -> Result<LoadedImage> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut decoder =
        Decoder::new(BufReader::new(file)).context("failed to create TIFF decoder")?;
    let mut pages = Vec::new();

    loop {
        let dimensions = decoder
            .dimensions()
            .context("failed to read TIFF dimensions")?;
        let color_type = decoder
            .colortype()
            .context("failed to read TIFF color type")?;
        let result = decoder.read_image().context("failed to decode TIFF page")?;
        let data = from_tiff_result(dimensions, color_type, result)?;
        pages.push(ImagePage { data });

        if !decoder.more_images() {
            break;
        }
        decoder
            .next_image()
            .context("failed to advance to next TIFF page")?;
    }

    if pages.is_empty() {
        bail!("TIFF file contains no pages");
    }

    Ok(LoadedImage {
        path: PathBuf::from(path),
        pages,
    })
}

fn from_tiff_result(
    (width, height): (u32, u32),
    color_type: ColorType,
    result: DecodingResult,
) -> Result<ImageData> {
    let samples = samples_for_color(color_type)?;
    let expected = width as usize * height as usize * samples;

    match (color_type, result) {
        (ColorType::Gray(_), DecodingResult::U8(data)) => check_len(
            ImageData::Gray8 {
                width,
                height,
                data,
            },
            expected,
        ),
        (ColorType::Gray(_), DecodingResult::U16(data)) => check_len(
            ImageData::Gray16 {
                width,
                height,
                data,
            },
            expected,
        ),
        (ColorType::Gray(_), DecodingResult::F32(data)) => check_len(
            ImageData::Gray32F {
                width,
                height,
                data,
            },
            expected,
        ),
        (ColorType::RGB(_), DecodingResult::U8(data)) => check_len(
            ImageData::Rgb8 {
                width,
                height,
                data,
            },
            expected,
        ),
        (ColorType::RGB(_), DecodingResult::U16(data)) => check_len(
            ImageData::Rgb16 {
                width,
                height,
                data,
            },
            expected,
        ),
        (ColorType::RGB(_), DecodingResult::F32(data)) => check_len(
            ImageData::Rgb32F {
                width,
                height,
                data,
            },
            expected,
        ),
        (ColorType::RGBA(_), DecodingResult::U8(data)) => check_len(
            ImageData::Rgba8 {
                width,
                height,
                data,
            },
            expected,
        ),
        (ColorType::RGBA(_), DecodingResult::U16(data)) => check_len(
            ImageData::Rgba16 {
                width,
                height,
                data,
            },
            expected,
        ),
        (ColorType::RGBA(_), DecodingResult::F32(data)) => check_len(
            ImageData::Rgba32F {
                width,
                height,
                data,
            },
            expected,
        ),
        (color_type, result) => Err(anyhow!(
            "unsupported TIFF combination: color={color_type:?}, sample={}",
            decoding_result_name(&result)
        )),
    }
}

fn samples_for_color(color_type: ColorType) -> Result<usize> {
    match color_type {
        ColorType::Gray(_) => Ok(1),
        ColorType::RGB(_) => Ok(3),
        ColorType::RGBA(_) => Ok(4),
        other => bail!("unsupported TIFF color type: {other:?}"),
    }
}

fn check_len(image: ImageData, expected: usize) -> Result<ImageData> {
    let actual = match &image {
        ImageData::Gray8 { data, .. } => data.len(),
        ImageData::Gray16 { data, .. } => data.len(),
        ImageData::Gray32F { data, .. } => data.len(),
        ImageData::Rgb8 { data, .. } => data.len(),
        ImageData::Rgb16 { data, .. } => data.len(),
        ImageData::Rgb32F { data, .. } => data.len(),
        ImageData::Rgba8 { data, .. } => data.len(),
        ImageData::Rgba16 { data, .. } => data.len(),
        ImageData::Rgba32F { data, .. } => data.len(),
    };

    if actual != expected {
        bail!("decoded image length mismatch: expected {expected}, got {actual}");
    }
    Ok(image)
}

fn decoding_result_name(result: &DecodingResult) -> &'static str {
    match result {
        DecodingResult::U8(_) => "u8",
        DecodingResult::U16(_) => "u16",
        DecodingResult::U32(_) => "u32",
        DecodingResult::U64(_) => "u64",
        DecodingResult::I8(_) => "i8",
        DecodingResult::I16(_) => "i16",
        DecodingResult::I32(_) => "i32",
        DecodingResult::I64(_) => "i64",
        DecodingResult::F32(_) => "f32",
        DecodingResult::F64(_) => "f64",
    }
}
