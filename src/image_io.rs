// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, DynamicImage, ImageFormat, ImageReader};
use tiff::decoder::{Decoder, DecodingResult};
use tiff::ColorType;

use crate::pixel::{ImageData, ImagePage, LoadedImage};

pub fn load_image(path: &Path) -> Result<LoadedImage> {
    // Detect the format from the file contents (falling back to the extension)
    // so multipage and animated formats are routed to the right decoder
    // regardless of how the file is named.
    let reader = ImageReader::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("failed to detect format of {}", path.display()))?;

    match reader.format() {
        Some(ImageFormat::Tiff) => load_tiff(path),
        Some(ImageFormat::Gif) => {
            let decoder = GifDecoder::new(reader.into_inner()).context("failed to read GIF")?;
            with_pages(path, frames_to_pages(decoder)?)
        }
        Some(ImageFormat::WebP) => {
            let decoder = WebPDecoder::new(reader.into_inner()).context("failed to read WebP")?;
            if decoder.has_animation() {
                with_pages(path, frames_to_pages(decoder)?)
            } else {
                let image = DynamicImage::from_decoder(decoder).context("failed to read WebP")?;
                single_page(path, from_dynamic(image)?)
            }
        }
        Some(ImageFormat::Png) => {
            let decoder = PngDecoder::new(reader.into_inner()).context("failed to read PNG")?;
            if decoder.is_apng().unwrap_or(false) {
                let decoder = decoder.apng().context("failed to read APNG")?;
                with_pages(path, frames_to_pages(decoder)?)
            } else {
                let image = DynamicImage::from_decoder(decoder).context("failed to read PNG")?;
                single_page(path, from_dynamic(image)?)
            }
        }
        _ => load_dynamic(path),
    }
}

fn load_dynamic(path: &Path) -> Result<LoadedImage> {
    let image = image::open(path).with_context(|| format!("failed to read {}", path.display()))?;
    single_page(path, from_dynamic(image)?)
}

fn single_page(path: &Path, data: ImageData) -> Result<LoadedImage> {
    Ok(LoadedImage {
        path: PathBuf::from(path),
        pages: vec![ImagePage {
            data,
            delay_ms: None,
        }],
    })
}

fn with_pages(path: &Path, pages: Vec<ImagePage>) -> Result<LoadedImage> {
    if pages.is_empty() {
        bail!("image contains no frames");
    }
    Ok(LoadedImage {
        path: PathBuf::from(path),
        pages,
    })
}

fn frames_to_pages<'a>(decoder: impl AnimationDecoder<'a>) -> Result<Vec<ImagePage>> {
    let frames = decoder
        .into_frames()
        .collect_frames()
        .context("failed to decode animation frames")?;

    Ok(frames
        .into_iter()
        .map(|frame| {
            let (numer, denom) = frame.delay().numer_denom_ms();
            let delay_ms = (denom != 0).then(|| numer / denom);
            let buffer = frame.into_buffer();
            let (width, height) = (buffer.width(), buffer.height());
            ImagePage {
                data: ImageData::Rgba8 {
                    width,
                    height,
                    data: buffer.into_raw(),
                },
                delay_ms,
            }
        })
        .collect())
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
        pages.push(ImagePage {
            data,
            delay_ms: None,
        });

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

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::GifEncoder;
    use image::{Delay, Frame, RgbaImage};

    #[test]
    fn loads_animated_gif_frames_as_pages() {
        let mut file = tempfile::Builder::new()
            .suffix(".gif")
            .tempfile()
            .expect("temp file");

        let frames = (0..3).map(|index| {
            let buffer = RgbaImage::from_pixel(4, 4, image::Rgba([index * 40, 0, 0, 255]));
            Frame::from_parts(buffer, 0, 0, Delay::from_numer_denom_ms(100, 1))
        });
        {
            let mut encoder = GifEncoder::new(file.as_file_mut());
            encoder.encode_frames(frames).expect("encode gif frames");
        }

        let loaded = load_image(file.path()).expect("load gif");
        assert_eq!(loaded.pages.len(), 3);
        for page in &loaded.pages {
            assert_eq!(page.data.dimensions(), (4, 4));
            assert_eq!(page.delay_ms, Some(100));
        }
    }
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
