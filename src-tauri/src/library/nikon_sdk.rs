//! Local bridge to the separately licensed Nikon Image SDK.
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn canonical_path(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    let path = path.as_ref().canonicalize()?;
    #[cfg(windows)]
    {
        // The native SDK helper compares Win32 paths, without Rust's verbatim prefix.
        let text = path.to_string_lossy();
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{unc}")));
        }
        if let Some(drive) = text.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(drive));
        }
    }
    Ok(path)
}

#[derive(Clone, Copy, Deserialize, Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum RenderMode {
    AsShot,
    SavedEdits,
}
impl RenderMode {
    fn name(self) -> &'static str {
        match self {
            Self::AsShot => "as-shot",
            Self::SavedEdits => "saved-edits",
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelperReport {
    schema_version: u32,
    sdk_loaded: bool,
    initialized: bool,
    input_opened: bool,
    rendered: bool,
    width: u32,
    height: u32,
    byte_depth: u32,
    color: u32,
    orientation: u32,
    output_path: PathBuf,
    color_space: String,
    settings: String,
    edit_state: Option<u32>,
}
fn parse_report(bytes: &[u8], mode: RenderMode, expected: &Path) -> Result<HelperReport, String> {
    let line = bytes
        .split(|b| *b == b'\n')
        .rev()
        .find(|line| !line.iter().all(u8::is_ascii_whitespace))
        .ok_or("显影程序未返回结果")?;
    let value: serde_json::Value =
        serde_json::from_slice(line).map_err(|_| "显影程序返回格式错误")?;
    if let Some(error) = value.get("error").and_then(serde_json::Value::as_str) {
        return Err(format!("Nikon SDK：{error}"));
    }
    let report: HelperReport = serde_json::from_value(value).map_err(|_| "显影结果字段不完整")?;
    if report.schema_version != 1
        || !report.sdk_loaded
        || !report.initialized
        || !report.input_opened
        || !report.rendered
        || report.settings != mode.name()
        || report.output_path != expected
        || report.color_space != "sRGB"
        || report.byte_depth != 1
        || report.color != 32
        || report.orientation != 1
        || report.width == 0
        || report.height == 0
        || u64::from(report.width) * u64::from(report.height) * 3 > 512 * 1024 * 1024
        || (matches!(mode, RenderMode::SavedEdits) && report.edit_state != Some(1))
    {
        return Err("显影结果未满足所选模式、色彩空间或图像完整性要求".into());
    }
    Ok(report)
}
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    output_path: String,
    width: u32,
    height: u32,
    settings: RenderMode,
    color_space: &'static str,
    cloud_style_verified: bool,
}

fn save_png(
    ppm: &Path,
    output: &Path,
    profile: &[u8],
    report: &HelperReport,
) -> Result<(), String> {
    if ppm.metadata().map_err(|e| e.to_string())?.len() > 512 * 1024 * 1024 + 1024 {
        return Err("显影输出超过大小限制".into());
    }
    let pixels = image::ImageReader::open(ppm)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    if pixels.width() != report.width
        || pixels.height() != report.height
        || pixels.color() != image::ColorType::Rgb8
    {
        return Err("显影文件与返回的尺寸或像素格式不一致".into());
    }
    let mut temporary = tempfile::NamedTempFile::new_in(output.parent().ok_or("无效的输出目录")?)
        .map_err(|e| e.to_string())?;
    let mut encoder = image::codecs::png::PngEncoder::new(temporary.as_file_mut());
    if !profile.is_empty() {
        encoder
            .set_icc_profile(profile.to_vec())
            .map_err(|e| e.to_string())?;
    }
    encoder
        .write_image(
            pixels.as_bytes(),
            report.width,
            report.height,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary
        .persist_noclobber(output)
        .map_err(|e| format!("无法保存新文件（不会覆盖已有文件）：{e}"))?;
    Ok(())
}
#[tauri::command]
pub async fn render_nikon_raw(
    sdk_directory: String,
    input_path: String,
    output_path: String,
    mode: RenderMode,
) -> Result<RenderResult, String> {
    let sdk = canonical_path(sdk_directory).map_err(|_| "找不到 Image SDK 文件夹")?;
    let helper = sdk.join("nikon-image-probe.exe");
    if !helper.is_file() || !sdk.join("NkImgSDK.dll").is_file() {
        return Err("请选择 Image SDK 的 Library/win/Bin/x64/Release 文件夹，并将 nikon-image-probe.exe 放在该目录。准备方法见 tools/nikon-image/README.md".into());
    }
    let input = canonical_path(input_path).map_err(|_| "找不到原始照片")?;
    let extension = input
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !input.is_file() || !matches!(extension.as_str(), "nef" | "nrw") {
        return Err("请选择 NEF 或 NRW 原始照片".into());
    }
    let requested = PathBuf::from(output_path);
    if requested
        .extension()
        .and_then(|s| s.to_str())
        .is_none_or(|s| !s.eq_ignore_ascii_case("png"))
    {
        return Err("输出文件须使用 .png 扩展名".into());
    }
    let parent = canonical_path(requested.parent().ok_or("请选择完整输出路径")?)
        .map_err(|_| "输出目录不存在")?;
    let output = parent.join(requested.file_name().ok_or("无效的输出文件名")?);
    if output.exists() {
        return Err("输出文件已存在，请选择新文件名；原文件不会被覆盖".into());
    }
    let icc = sdk.join("../../../Profiles/NKsRGB.icm");
    if icc
        .metadata()
        .map_err(|_| "缺少 SDK 的官方 sRGB 色彩配置")?
        .len()
        > 4 * 1024 * 1024
    {
        return Err("色彩配置文件异常".into());
    }
    let profile = std::fs::read(icc).map_err(|e| e.to_string())?;
    if profile.len() < 128 || &profile[36..40] != b"acsp" {
        return Err("官方 sRGB ICC 配置格式无效".into());
    }
    let scratch = tempfile::Builder::new()
        .prefix(".nikon-render-")
        .tempdir_in(&parent)
        .map_err(|e| e.to_string())?;
    let ppm = scratch.path().join("render.ppm");
    let mut command = tokio::process::Command::new(helper);
    command.arg(&sdk).arg(&input).arg(&ppm).kill_on_drop(true);
    if matches!(mode, RenderMode::SavedEdits) {
        command.arg("--saved-edits");
    }
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let process = tokio::time::timeout(Duration::from_secs(180), command.output())
        .await
        .map_err(|_| "官方显影超时，辅助进程已停止")?
        .map_err(|e| format!("无法启动显影工具：{e}"))?;
    let report = parse_report(&process.stdout, mode, &ppm)?;
    if !process.status.success() {
        return Err("显影程序异常结束，未保存输出".into());
    }
    tokio::task::spawn_blocking(move || {
        save_png(&ppm, &output, &profile, &report)?;
        drop(scratch);
        Ok(RenderResult {
            output_path: output.to_string_lossy().into_owned(),
            width: report.width,
            height: report.height,
            settings: mode,
            color_space: "sRGB",
            cloud_style_verified: false,
        })
    })
    .await
    .map_err(|e| format!("图像保存任务失败：{e}"))?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Requires separately licensed local Nikon SDK and an explicit NEF/output path"]
    async fn real_sdk_png_render_preserves_source() {
        let sdk = std::env::var("NPS_IMAGE_SDK_BIN").expect("Set NPS_IMAGE_SDK_BIN");
        let input = std::env::var("NPS_TEST_NEF").expect("Set NPS_TEST_NEF");
        let output = std::env::var("NPS_TEST_OUTPUT").expect("Set NPS_TEST_OUTPUT to a new PNG");
        let source = std::fs::read(&input).unwrap();
        let result = render_nikon_raw(
            sdk.clone(),
            input.clone(),
            output.clone(),
            RenderMode::AsShot,
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&input).unwrap(), source);
        let decoded = image::open(&output).unwrap();
        assert_eq!(
            (decoded.width(), decoded.height()),
            (result.width, result.height)
        );
        assert!(!result.cloud_style_verified);
        let png_before = std::fs::read(&output).unwrap();
        assert!(
            render_nikon_raw(sdk, input, output.clone(), RenderMode::AsShot)
                .await
                .unwrap_err()
                .contains("已存在")
        );
        assert_eq!(std::fs::read(output).unwrap(), png_before);
        println!("{}", serde_json::to_string(&result).unwrap());
    }
    fn valid() -> serde_json::Value {
        serde_json::json!({"schemaVersion":1,"sdkLoaded":true,"initialized":true,"inputOpened":true,"rendered":true,"width":2,"height":1,"byteDepth":1,"color":32,"orientation":1,"outputPath":"render.ppm","colorSpace":"sRGB","settings":"as-shot"})
    }
    #[test]
    fn refuses_fallback_and_unverified_saved_edit_state() {
        let mut value = valid();
        assert!(parse_report(
            value.to_string().as_bytes(),
            RenderMode::AsShot,
            Path::new("render.ppm")
        )
        .is_ok());
        assert!(parse_report(
            value.to_string().as_bytes(),
            RenderMode::SavedEdits,
            Path::new("render.ppm")
        )
        .is_err());
        value["settings"] = "saved-edits".into();
        assert!(parse_report(
            value.to_string().as_bytes(),
            RenderMode::SavedEdits,
            Path::new("render.ppm")
        )
        .is_err());
        value["editState"] = 1.into();
        assert!(parse_report(
            value.to_string().as_bytes(),
            RenderMode::SavedEdits,
            Path::new("render.ppm")
        )
        .is_ok());
        value["error"] = "fallback warning".into();
        assert!(parse_report(
            value.to_string().as_bytes(),
            RenderMode::SavedEdits,
            Path::new("render.ppm")
        )
        .is_err());
    }
    #[test]
    fn rejects_unexpected_output_and_incomplete_render() {
        for (key, bad) in [
            ("outputPath", serde_json::json!("other.ppm")),
            ("rendered", serde_json::json!(false)),
            ("width", serde_json::json!(0)),
            ("orientation", serde_json::json!(6)),
            ("colorSpace", serde_json::json!("unknown")),
        ] {
            let mut value = valid();
            value[key] = bad;
            assert!(parse_report(
                value.to_string().as_bytes(),
                RenderMode::AsShot,
                Path::new("render.ppm")
            )
            .is_err());
        }
    }
    #[test]
    fn png_conversion_preserves_pixels_and_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let ppm = directory.path().join("render.ppm");
        let png = directory.path().join("result.png");
        let pixels = [10, 20, 30, 200, 150, 100];
        let mut bytes = b"P6\n2 1\n255\n".to_vec();
        bytes.extend(pixels);
        std::fs::write(&ppm, bytes).unwrap();
        let report = parse_report(
            valid().to_string().as_bytes(),
            RenderMode::AsShot,
            Path::new("render.ppm"),
        )
        .unwrap();
        save_png(&ppm, &png, &[], &report).unwrap();
        assert_eq!(image::open(&png).unwrap().to_rgb8().as_raw(), &pixels);
        let before = std::fs::read(&png).unwrap();
        assert!(save_png(&ppm, &png, &[], &report).is_err());
        assert_eq!(std::fs::read(&png).unwrap(), before);
    }
}
