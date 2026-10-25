use super::parser;
use super::{Preset, PresetFormat, PresetParams, PresetSource};

#[tauri::command]
pub async fn inspect_nikon_file(path: String) -> Result<super::nikon_file::NikonInspection, String> {
    super::nikon_file::NikonFile::read(std::path::Path::new(&path))
        .map(|file| file.inspection()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod import_safety_tests {
    #[tokio::test]
    async fn unverified_nikon_binary_is_not_imported_as_neutral_parameters() {
        let dir = tempfile::tempdir().unwrap();
        for extension in ["np3", "NP3", "ncp"] {
            let path = dir.path().join(format!("unverified.{extension}"));
            let original = b"NCP\0not-a-validated-picture-control";
            std::fs::write(&path, original).unwrap();
            let result = super::import_preset(path.to_string_lossy().into_owned()).await;
            assert!(result.unwrap_err().contains("完整解析尚未验证"));
            assert_eq!(std::fs::read(path).unwrap(), original);
        }
    }
}

#[tauri::command]
pub async fn import_preset(path: String) -> Result<Preset, String> {
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .ok_or("No file extension")?;

    let format = PresetFormat::from_extension(ext)
        .ok_or_else(|| format!("Unsupported preset format: {}", ext))?;

    // The historical byte-scanning decoder does not establish Nikon's base
    // profile, curves or Flexible Color semantics. Do not expose its guesses
    // as a faithfully rendered preset through the production import command.
    if matches!(format, PresetFormat::NP3 | PresetFormat::NCP) {
        return Err("Nikon NP3/NCP 完整解析尚未验证，不能作为电脑调色预设应用；原文件保持不变。云创公开参数可在预设管理中单独查看。".into());
    }

    let params = parser::parse_preset_file(&path, &format)
        .map_err(|e| format!("Parse failed: {}", e))?;

    // Prefer an embedded name (Nikon control name / XMP dc:title), else the file stem.
    let name = parser::detect_preset_name(&path, &format).unwrap_or_else(|| {
        std::path::Path::new(&path)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("Unnamed Preset")
            .to_string()
    });

    let preset = Preset {
        id: 0, // Will be assigned by DB
        name,
        format,
        params,
        lut_path: if ext == "cube" { Some(path.clone()) } else { None },
        source: PresetSource::Imported,
        created_at: "2026-07-21T00:00:00Z".to_string(),
    };

    // TODO: Save to SQLite database
    Ok(preset)
}

#[tauri::command]
pub async fn list_presets() -> Result<Vec<Preset>, String> {
    // TODO: Query from SQLite database
    Ok(vec![])
}

#[tauri::command]
pub async fn apply_preset(
    photo_path: String,
    preset_params: PresetParams,
) -> Result<String, String> {
    // Non-destructive: save params to .nps sidecar file
    let sidecar_path = format!("{}.nps", photo_path);

    let sidecar = serde_json::json!({
        "version": 1,
        "original_file": photo_path,
        "params": preset_params,
        "applied_at": "2026-07-21T00:00:00Z"
    });

    std::fs::write(&sidecar_path, serde_json::to_string_pretty(&sidecar).unwrap())
        .map_err(|e| format!("Failed to write sidecar: {}", e))?;

    Ok(sidecar_path)
}

#[tauri::command]
pub async fn export_with_preset(
    photo_path: String,
    preset_params: PresetParams,
    output_path: String,
    quality: u8,
) -> Result<String, String> {
    // Load image
    let img = image::open(&photo_path).map_err(|e| format!("Failed to open: {}", e))?;

    // Apply preset params to image
    let mut output = img.to_rgb8();
    let (w, h) = output.dimensions();

    for y in 0..h {
        for x in 0..w {
            let pixel = output.get_pixel(x, y);
            let r = pixel[0] as f32 / 255.0;
            let g = pixel[1] as f32 / 255.0;
            let b = pixel[2] as f32 / 255.0;

            let (nr, ng, nb) = preset_params.apply_to_pixel(r, g, b);

            output.put_pixel(x, y, image::Rgb([
                (nr * 255.0) as u8,
                (ng * 255.0) as u8,
                (nb * 255.0) as u8,
            ]));
        }
    }

    // Save as JPEG with quality
    let file = std::fs::File::create(&output_path)
        .map_err(|e| format!("Failed to create output: {}", e))?;
    let mut writer = std::io::BufWriter::new(file);

    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, quality);
    encoder
        .encode_image(&output)
        .map_err(|e| format!("Encode failed: {}", e))?;

    Ok(output_path)
}
