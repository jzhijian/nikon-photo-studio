//! Final photo export: full RAW/raster decode, output sizing, current develop
//! adjustments and JPEG/PNG/TIFF encoding.

use image::{DynamicImage, EncodableLayout, GenericImageView, ImageEncoder};
use serde::{Deserialize, Serialize};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportAdjust {
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub temperature: f32,
    pub tint: f32,
    pub vibrance: f32,
    pub saturation: f32,
    pub clarity: f32,
    pub texture: f32,
    pub dehaze: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    pub format: String,
    pub bit_depth: u8,
    pub quality: u8,
    pub resize_mode: String,
    pub width: u32,
    pub height: u32,
    pub long_edge: u32,
    pub no_upscale: bool,
    pub dpi: u32,
    pub output_dir: String,
    pub suffix: String,
    pub overwrite: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub output_path: String,
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub format: String,
    pub file_size: u64,
}

fn output_dimensions(source: (u32, u32), options: &ExportOptions) -> Result<(u32, u32), String> {
    let (source_width, source_height) = source;
    if source_width == 0 || source_height == 0 {
        return Err("source image has invalid dimensions".into());
    }
    let requested = match options.resize_mode.as_str() {
        "original" => (source_width, source_height),
        "long-edge" => {
            let edge = options.long_edge.clamp(1, 32_768);
            let scale = edge as f64 / source_width.max(source_height) as f64;
            (
                (source_width as f64 * scale).round() as u32,
                (source_height as f64 * scale).round() as u32,
            )
        }
        "dimensions" => {
            let bound_width = options.width.clamp(1, 32_768);
            let bound_height = options.height.clamp(1, 32_768);
            let scale = (bound_width as f64 / source_width as f64)
                .min(bound_height as f64 / source_height as f64);
            (
                (source_width as f64 * scale).round() as u32,
                (source_height as f64 * scale).round() as u32,
            )
        }
        value => return Err(format!("unsupported resize mode: {value}")),
    };
    if options.no_upscale && (requested.0 > source_width || requested.1 > source_height) {
        Ok((source_width, source_height))
    } else {
        Ok((requested.0.max(1), requested.1.max(1)))
    }
}

fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn white_balance(temperature: f32, tint: f32) -> (f32, f32, f32) {
    let temp = ((temperature - 5500.0) / 5500.0).clamp(-0.75, 0.8);
    let tint = (tint / 100.0).clamp(-1.0, 1.0);
    (
        (1.0 + temp * 0.22 + tint * 0.05).max(0.25),
        (1.0 - tint * 0.08).max(0.25),
        (1.0 - temp * 0.22 + tint * 0.05).max(0.25),
    )
}

fn process_pixel(r: f32, g: f32, b: f32, adjust: &ExportAdjust) -> [f32; 3] {
    let (wr, wg, wb) = white_balance(adjust.temperature, adjust.tint);
    let exposure = 2.0f32.powf(adjust.exposure);
    let mut values = [r * wr * exposure, g * wg * exposure, b * wb * exposure];

    let luminance = 0.299 * values[0] + 0.587 * values[1] + 0.114 * values[2];
    let shadow_weight = (1.0 - luminance * 2.0).clamp(0.0, 1.0);
    let highlight_weight = ((luminance - 0.5) * 2.0).clamp(0.0, 1.0);
    let tone_shift = adjust.shadows / 100.0 * shadow_weight * 0.35
        + adjust.highlights / 100.0 * highlight_weight * 0.35
        + adjust.whites / 100.0 * highlight_weight * 0.18
        + adjust.blacks / 100.0 * shadow_weight * 0.18;
    if luminance > 0.0001 {
        let scale = clamp01(luminance + tone_shift) / luminance;
        values.iter_mut().for_each(|value| *value *= scale);
    }

    let contrast = (1.0 + adjust.contrast / 100.0 + adjust.dehaze / 240.0).max(0.0);
    values
        .iter_mut()
        .for_each(|value| *value = (*value - 0.5) * contrast + 0.5);

    let gray = 0.299 * values[0] + 0.587 * values[1] + 0.114 * values[2];
    let max = values[0].max(values[1]).max(values[2]);
    let min = values[0].min(values[1]).min(values[2]);
    let current_saturation = if max > 0.0001 { (max - min) / max } else { 0.0 };
    let saturation =
        (1.0 + adjust.saturation / 100.0 + adjust.vibrance / 100.0 * (1.0 - current_saturation))
            .max(0.0);
    values
        .iter_mut()
        .for_each(|value| *value = gray + (*value - gray) * saturation);
    [clamp01(values[0]), clamp01(values[1]), clamp01(values[2])]
}

fn apply_adjustments(image: DynamicImage, adjust: &ExportAdjust, depth: u8) -> DynamicImage {
    if depth == 16 {
        let mut output = image.to_rgb16();
        for pixel in output.pixels_mut() {
            let result = process_pixel(
                pixel[0] as f32 / 65_535.0,
                pixel[1] as f32 / 65_535.0,
                pixel[2] as f32 / 65_535.0,
                adjust,
            );
            *pixel = image::Rgb([
                (result[0] * 65_535.0).round() as u16,
                (result[1] * 65_535.0).round() as u16,
                (result[2] * 65_535.0).round() as u16,
            ]);
        }
        DynamicImage::ImageRgb16(output)
    } else {
        let mut output = image.to_rgb8();
        for pixel in output.pixels_mut() {
            let result = process_pixel(
                pixel[0] as f32 / 255.0,
                pixel[1] as f32 / 255.0,
                pixel[2] as f32 / 255.0,
                adjust,
            );
            *pixel = image::Rgb([
                (result[0] * 255.0).round() as u8,
                (result[1] * 255.0).round() as u8,
                (result[2] * 255.0).round() as u8,
            ]);
        }
        DynamicImage::ImageRgb8(output)
    }
}

fn available_path(
    directory: &Path,
    stem: &str,
    extension: &str,
    overwrite: bool,
) -> Result<PathBuf, String> {
    let initial = directory.join(format!("{stem}.{extension}"));
    if overwrite || !initial.exists() {
        return Ok(initial);
    }
    for index in 1..10_000 {
        let candidate = directory.join(format!("{stem}-{index}.{extension}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("cannot allocate a unique output filename".into())
}

fn encode(image: &DynamicImage, path: &Path, options: &ExportOptions) -> Result<(), String> {
    let file =
        std::fs::File::create(path).map_err(|error| format!("create export failed: {error}"))?;
    let writer = BufWriter::new(file);
    let (width, height) = image.dimensions();
    match options.format.as_str() {
        "jpeg" => image::codecs::jpeg::JpegEncoder::new_with_quality(
            writer,
            options.quality.clamp(1, 100),
        )
        .write_image(
            image.to_rgb8().as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        ),
        "png" if options.bit_depth == 16 => image::codecs::png::PngEncoder::new(writer)
            .write_image(
                image.to_rgb16().as_bytes(),
                width,
                height,
                image::ExtendedColorType::Rgb16,
            ),
        "png" => image::codecs::png::PngEncoder::new(writer).write_image(
            image.to_rgb8().as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        ),
        "tiff" if options.bit_depth == 16 => image::codecs::tiff::TiffEncoder::new(writer)
            .write_image(
                image.to_rgb16().as_bytes(),
                width,
                height,
                image::ExtendedColorType::Rgb16,
            ),
        "tiff" => image::codecs::tiff::TiffEncoder::new(writer).write_image(
            image.to_rgb8().as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        ),
        value => return Err(format!("unsupported export format: {value}")),
    }
    .map_err(|error| format!("encode export failed: {error}"))
}

#[tauri::command]
pub async fn export_photo(
    photo_path: String,
    adjust: ExportAdjust,
    options: ExportOptions,
) -> Result<ExportResult, String> {
    tokio::task::spawn_blocking(move || {
        let output_dir = PathBuf::from(&options.output_dir);
        std::fs::create_dir_all(&output_dir)
            .map_err(|error| format!("create output directory failed: {error}"))?;
        let format = options.format.to_ascii_lowercase();
        if !matches!(format.as_str(), "jpeg" | "png" | "tiff") {
            return Err(format!("unsupported export format: {format}"));
        }
        if format == "jpeg" && options.bit_depth != 8 {
            return Err("JPEG only supports 8-bit output".into());
        }
        if !matches!(options.bit_depth, 8 | 16) {
            return Err("bit depth must be 8 or 16".into());
        }
        let _dpi = options.dpi.clamp(72, 1200); // retained for future metadata writers; pixel dimensions define detail.

        let mut image = crate::library::commands::load_export_image(&photo_path)?;
        let dimensions = output_dimensions(image.dimensions(), &options)?;
        if dimensions != image.dimensions() {
            image = image.resize_exact(
                dimensions.0,
                dimensions.1,
                image::imageops::FilterType::Lanczos3,
            );
        }
        image = apply_adjustments(image, &adjust, options.bit_depth);
        let sharpen = (adjust.clarity.max(0.0) * 0.012 + adjust.texture.max(0.0) * 0.008).min(1.5);
        if sharpen > 0.01 {
            image = image.unsharpen(0.6 + sharpen, 2);
        }

        let source_stem = Path::new(&photo_path)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("photo");
        let safe_suffix: String = options
            .suffix
            .chars()
            .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
            .take(80)
            .collect();
        let stem = format!("{source_stem}{safe_suffix}");
        let extension = if format == "jpeg" {
            "jpg"
        } else if format == "tiff" {
            "tif"
        } else {
            "png"
        };
        let output_path = available_path(&output_dir, &stem, extension, options.overwrite)?;
        encode(
            &image,
            &output_path,
            &ExportOptions {
                format: format.clone(),
                ..options.clone()
            },
        )?;
        let file_size = std::fs::metadata(&output_path)
            .map_err(|error| format!("read export metadata failed: {error}"))?
            .len();
        Ok(ExportResult {
            output_path: output_path.to_string_lossy().into_owned(),
            width: dimensions.0,
            height: dimensions.1,
            bit_depth: options.bit_depth,
            format,
            file_size,
        })
    })
    .await
    .map_err(|error| format!("export task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(dir: &Path, format: &str, bit_depth: u8) -> ExportOptions {
        ExportOptions {
            format: format.into(),
            bit_depth,
            quality: 92,
            resize_mode: "long-edge".into(),
            width: 100,
            height: 100,
            long_edge: 64,
            no_upscale: true,
            dpi: 300,
            output_dir: dir.to_string_lossy().into_owned(),
            suffix: "-export".into(),
            overwrite: false,
        }
    }

    #[test]
    fn dimensions_preserve_aspect_ratio_and_no_upscale() {
        let mut value = options(Path::new("."), "jpeg", 8);
        assert_eq!(output_dimensions((400, 200), &value).unwrap(), (64, 32));
        value.long_edge = 800;
        assert_eq!(output_dimensions((400, 200), &value).unwrap(), (400, 200));
    }

    #[test]
    fn encodes_all_supported_formats_and_depths() {
        let dir = tempfile::tempdir().unwrap();
        let image = DynamicImage::ImageRgb16(
            image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_fn(32, 16, |x, y| {
                image::Rgb([x as u16 * 1000, y as u16 * 2000, 20_000])
            }),
        );
        for (format, depth) in [
            ("jpeg", 8),
            ("png", 8),
            ("png", 16),
            ("tiff", 8),
            ("tiff", 16),
        ] {
            let opts = options(dir.path(), format, depth);
            let extension = if format == "jpeg" {
                "jpg"
            } else if format == "tiff" {
                "tif"
            } else {
                "png"
            };
            let path = dir
                .path()
                .join(format!("test-{format}-{depth}.{extension}"));
            encode(&image, &path, &opts).unwrap();
            let decoded = image::open(path).unwrap();
            assert_eq!(decoded.dimensions(), (32, 16));
        }
    }
}
