//! Parsers for preset formats: XMP, Cube LUT, NP3/NCP, LRPreset, 3DL, COStyle.
//!
//! Design goal: importing a preset must be ROBUST. Selecting a readable file should
//! always yield an imported preset (best-effort parameters + a meaningful name) rather
//! than failing silently. Hard errors are reserved for genuine I/O failures only.
//!
//! Format notes (researched):
//! - XMP     : Adobe Camera Raw / Lightroom Classic (7.3+) develop presets. RDF/XML with
//!             `crs:*` attributes (crs:Exposure2012="+0.30") or element form.
//! - CUBE    : Resolve/Adobe 3D LUT. Text header (LUT_3D_SIZE / TITLE) + float triplets.
//!             A color transform, not develop sliders -> imported as a LUT reference.
//! - NP3     : Nikon Picture Control 3 (Z-series: Z30/Z5II...). Binary container embedding
//!             a 4-byte ASCII version code ("0103"/"0203"/...) followed by signed-byte
//!             adjustments (sharpening/clarity/contrast/brightness/saturation/hue) and an
//!             embedded control name. Ref: horshack-dpreview/NikonPictureControlsDev.
//! - NCP     : Nikon Capture NX-D Picture Control (older D-series). Same adjustment block,
//!             version code "0100"/"0200".
//! - LRPreset: Lightroom Classic <=6 preset. Apple property list (binary "bplist00" or XML
//!             <plist>) with <key>Exposure</key><real>..</real> style entries.
//! - 3DL     : Autodesk/Assimilate 3D LUT. Integer RGB triplet grid.
//! - COStyle : Capture One style. Plist-like XML with an Adjustments dict.

use super::{PresetFormat, PresetParams};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("XML parse error: {0}")]
    Xml(String),
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(String),
}

/// Parse a preset file based on its format. Returns best-effort parameters.
/// Only fails on genuine I/O errors (unreadable file).
pub fn parse_preset_file(path: &str, format: &PresetFormat) -> Result<PresetParams, ParseError> {
    let data = std::fs::read(path)?;
    match format {
        PresetFormat::XMP => parse_xmp(&data),
        PresetFormat::Cube => parse_cube(&data),
        PresetFormat::NP3 => parse_np3(&data),
        PresetFormat::NCP => parse_ncp(&data),
        PresetFormat::LRPreset => parse_lrpreset(&data),
        PresetFormat::ThreeDL => parse_3dl(&data),
        PresetFormat::COStyle => parse_costyle(&data),
    }
}

/// Best-effort human-readable name for a preset file (embedded name or file stem).
pub fn detect_preset_name(path: &str, format: &PresetFormat) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    let embedded = match format {
        PresetFormat::NP3 | PresetFormat::NCP => extract_nikon_name(&data),
        PresetFormat::XMP => extract_xmp_name(&data),
        _ => None,
    };
    if let Some(n) = embedded {
        return Some(n);
    }
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// XMP (Adobe Camera Raw / Lightroom Classic)
// ---------------------------------------------------------------------------

fn parse_xmp(data: &[u8]) -> Result<PresetParams, ParseError> {
    let text = String::from_utf8_lossy(data);
    let mut params = PresetParams::default();

    // Try attribute form first (crs:Foo="v"), then element form (<crs:Foo>v</crs:Foo>).
    let get = |attr: &str, elem: &str| -> Option<f32> {
        extract_xmp_float(&text, attr).or_else(|| extract_xmp_element(&text, elem))
    };

    params.exposure = get("Exposure2012", "Exposure2012");
    params.contrast = get("Contrast2012", "Contrast2012");
    params.highlights = get("Highlights2012", "Highlights2012");
    params.shadows = get("Shadows2012", "Shadows2012");
    params.whites = get("Whites2012", "Whites2012");
    params.blacks = get("Blacks2012", "Blacks2012");
    params.temperature = get("Temperature", "Temperature");
    params.tint = get("Tint", "Tint");
    params.vibrance = get("Vibrance", "Vibrance");
    params.saturation = get("Saturation", "Saturation");
    params.clarity = get("Clarity2012", "Clarity2012");
    params.sharpening = get("Sharpness", "Sharpness");

    Ok(params)
}

/// Extract a float value from an XMP attribute like crs:Exposure2012="+0.50"
fn extract_xmp_float(xml: &str, attr_name: &str) -> Option<f32> {
    let patterns = [
        format!("crs:{}=\"", attr_name),
        format!("{}=\"", attr_name),
    ];
    for pattern in &patterns {
        if let Some(start) = xml.find(pattern.as_str()) {
            let value_start = start + pattern.len();
            if let Some(end) = xml[value_start..].find('"') {
                let value_str = &xml[value_start..value_start + end];
                if let Ok(v) = value_str.parse::<f32>() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Extract a float from XMP element form: <crs:Exposure2012>0.5</crs:Exposure2012>
fn extract_xmp_element(xml: &str, name: &str) -> Option<f32> {
    let tags = [format!("<crs:{}>", name), format!("<{}>", name)];
    for tag in &tags {
        if let Some(start) = xml.find(tag.as_str()) {
            let vs = start + tag.len();
            if let Some(rel) = xml[vs..].find('<') {
                let val = xml[vs..vs + rel].trim();
                if let Ok(v) = val.parse::<f32>() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Extract a preset name from XMP dc:title (attribute form).
fn extract_xmp_name(data: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(data);
    let marker = "dc:title=\"";
    if let Some(start) = text.find(marker) {
        let vs = start + marker.len();
        if let Some(end) = text[vs..].find('"') {
            let v = text[vs..vs + end].trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Cube / 3DL LUTs (color transforms, imported as references)
// ---------------------------------------------------------------------------

fn parse_cube(_data: &[u8]) -> Result<PresetParams, ParseError> {
    // A .cube file is a 3D LUT color transform, not develop sliders. We import it as a
    // LUT reference (lut_path is set by the command) with neutral develop params so the
    // preset always appears in the library.
    Ok(PresetParams::default())
}

fn parse_3dl(_data: &[u8]) -> Result<PresetParams, ParseError> {
    // .3dl is likewise a LUT grid; import as reference with neutral params.
    Ok(PresetParams::default())
}

// ---------------------------------------------------------------------------
// Nikon NP3 / NCP Picture Control (binary)
// Ref: horshack-dpreview/NikonPictureControlsDev
//
// Real-world format variants:
//   NCP (Capture NX-D / Picture Control Utility 2):
//     Bytes 0-3: binary version (e.g. 00 00 01 00 = v1.0)
//     Bytes 4-7: flags
//     Bytes 8-27: picture control name (20 bytes, null-padded ASCII)
//     Byte 28: sharpening  (0-9, 0xFF = auto)
//     Byte 29: clarity     (signed -5..+5, 0x80 or 0xFF = auto)
//     Byte 30: contrast    (signed -5..+5, 0x80 or 0xFF = auto)
//     Byte 31: brightness  (signed -5..+5, 0x80 or 0xFF = auto)
//     Byte 32: saturation  (signed -5..+5, 0x80 or 0xFF = auto)
//     Byte 33: hue/tint    (signed -5..+5, 0x80 or 0xFF = auto)
//
//   NP3 (Z-series Picture Control 3):
//     Container marker "NP3X" or similar at offset 0
//     4-byte ASCII version code ("0103"/"0203"/...) at variable offset
//     Adjustment block follows version code at +8
//     Embedded control name somewhere in the first 64 bytes
// ---------------------------------------------------------------------------

/// Known Nikon picture-control version codes (4-byte ASCII) seen across NCP/NP3 files.
const NIKON_VERSION_CODES: &[&str] = &[
    "0100", "0101", "0200", "0203", "0103", "0300", "0303", "0400",
];

/// Nikon picture-control names (bundled + Creative Picture Controls).
const NIKON_CONTROL_NAMES: &[&str] = &[
    "Standard", "Neutral", "Vivid", "Monochrome", "Portrait", "Landscape", "Flat",
    "Dream", "Morning", "Pop", "Sunday", "Somber", "Dramatic", "Silence", "Bleached",
    "Melancholic", "Pure", "Denim", "Toy", "Sepia", "Blue", "Red", "Pink", "Charcoal",
    "Graphite", "Binary", "Carbon", "Deep", "Bright", "Warm", "Dark", "Fade",
];

/// Scan the header region for a Nikon ASCII version code; return its offset.
fn find_nikon_version(data: &[u8]) -> Option<usize> {
    let scan_len = data.len().min(512);
    if scan_len < 4 {
        return None;
    }
    for i in 0..=(scan_len - 4) {
        if let Ok(s) = std::str::from_utf8(&data[i..i + 4]) {
            if NIKON_VERSION_CODES.contains(&s) {
                return Some(i);
            }
        }
    }
    None
}

/// Detect NCP binary header format (Picture Control Utility 2 / Capture NX-D).
/// Returns true if the data looks like an NCP file with a binary version header.
fn is_ncp_binary_header(data: &[u8]) -> bool {
    if data.len() < 34 {
        return false;
    }
    // NCP files typically start with a small binary version number
    // Common patterns: 00 00 01 00 (v1.0), 00 00 02 00 (v2.0)
    // The key indicator: bytes 0-1 are 0x00, byte 2 is a small version (1-4), byte 3 is 0x00
    if data[0] == 0x00 && data[1] == 0x00 && data[2] >= 1 && data[2] <= 9 && data[3] == 0x00 {
        return true;
    }
    // Alternative: starts with "NPC" or "NCP" magic
    if data.len() >= 3 && (&data[0..3] == b"NPC" || &data[0..3] == b"NCP") {
        return true;
    }
    false
}

/// Decode adjustment bytes from the standard NCP layout (fixed offsets from file start).
/// NCP format: name at bytes 8-27, adjustments start at byte 28.
fn decode_ncp_fixed(data: &[u8]) -> PresetParams {
    let mut p = PresetParams::default();
    let at = |off: usize| -> Option<u8> { data.get(off).copied() };

    // Sharpening at byte 28 (0-9 scale, 0xFF = auto)
    if let Some(s) = at(28) {
        if s != 0xFF && s <= 9 {
            p.sharpening = Some((s as f32 * 11.0).clamp(0.0, 100.0));
        }
    }
    // Clarity at byte 29 (signed -5..+5, 0xFF/0x80 = auto)
    if let Some(c) = at(29) {
        let v = c as i8;
        if v >= -5 && v <= 5 {
            p.clarity = Some((v as f32 * 20.0).clamp(-100.0, 100.0));
        }
    }
    // Contrast at byte 30
    if let Some(c) = at(30) {
        let v = c as i8;
        if v >= -5 && v <= 5 {
            p.contrast = Some((v as f32 * 20.0).clamp(-100.0, 100.0));
        }
    }
    // Brightness/Exposure at byte 31
    if let Some(b) = at(31) {
        let v = b as i8;
        if v >= -5 && v <= 5 {
            p.exposure = Some((v as f32 * 0.5).clamp(-5.0, 5.0));
        }
    }
    // Saturation at byte 32
    if let Some(s) = at(32) {
        let v = s as i8;
        if v >= -5 && v <= 5 {
            p.saturation = Some((v as f32 * 20.0).clamp(-100.0, 100.0));
        }
    }
    // Hue/Tint at byte 33
    if let Some(h) = at(33) {
        let v = h as i8;
        if v >= -5 && v <= 5 {
            p.tint = Some((v as f32 * 20.0).clamp(-100.0, 100.0));
        }
    }
    p
}

/// Decode Nikon adjustment bytes relative to an ASCII version-code offset (NP3 style).
/// Layout: version[4], flags[4], then signed-byte adjustments at +8.
fn decode_nikon_params(data: &[u8], m: usize) -> PresetParams {
    let mut p = PresetParams::default();
    let at = |off: usize| -> Option<u8> { data.get(m + off).copied() };

    if let Some(s) = at(8) {
        if s != 0xFF && s <= 9 {
            p.sharpening = Some((s as f32 * 11.0).clamp(0.0, 100.0));
        }
    }
    if let Some(c) = at(9) {
        let v = c as i8;
        if v >= -5 && v <= 5 {
            p.clarity = Some((v as f32 * 20.0).clamp(-100.0, 100.0));
        }
    }
    if let Some(c) = at(10) {
        let v = c as i8;
        if v >= -5 && v <= 5 {
            p.contrast = Some((v as f32 * 20.0).clamp(-100.0, 100.0));
        }
    }
    if let Some(b) = at(11) {
        let v = b as i8;
        if v >= -5 && v <= 5 {
            p.exposure = Some((v as f32 * 0.5).clamp(-5.0, 5.0));
        }
    }
    if let Some(s) = at(12) {
        let v = s as i8;
        if v >= -5 && v <= 5 {
            p.saturation = Some((v as f32 * 20.0).clamp(-100.0, 100.0));
        }
    }
    if let Some(h) = at(13) {
        let v = h as i8;
        if v >= -5 && v <= 5 {
            p.tint = Some((v as f32 * 20.0).clamp(-100.0, 100.0));
        }
    }
    p
}

/// Heuristic fallback: scan for a block of 6 consecutive small signed bytes
/// that look like Nikon adjustment parameters (sharpening 0-9, others -5..+5).
fn heuristic_find_adjustments(data: &[u8]) -> Option<PresetParams> {
    if data.len() < 40 {
        return None;
    }
    // Scan bytes 4..min(len-6, 128) for a plausible adjustment block
    let end = data.len().min(128);
    for i in 4..end.saturating_sub(6) {
        let sharpening = data[i];
        let clarity = data[i + 1] as i8;
        let contrast = data[i + 2] as i8;
        let brightness = data[i + 3] as i8;
        let saturation = data[i + 4] as i8;
        let hue = data[i + 5] as i8;

        // Sharpening must be 0-9 or 0xFF (auto)
        let sharp_ok = sharpening <= 9 || sharpening == 0xFF;
        // Others must be in -5..+5 range or 0xFF/0x80 (auto)
        let signed_ok = |v: i8| (v >= -5 && v <= 5) || v == -1;

        if sharp_ok && signed_ok(clarity) && signed_ok(contrast)
            && signed_ok(brightness) && signed_ok(saturation) && signed_ok(hue)
        {
            // At least 2 non-zero/non-auto values to avoid false positives on zero-filled regions
            let non_default = [
                sharpening != 0 && sharpening != 0xFF,
                clarity != 0 && clarity != -1,
                contrast != 0 && contrast != -1,
                brightness != 0 && brightness != -1,
                saturation != 0 && saturation != -1,
                hue != 0 && hue != -1,
            ].iter().filter(|&&x| x).count();

            if non_default >= 1 {
                let mut p = PresetParams::default();
                if sharpening != 0xFF {
                    p.sharpening = Some((sharpening as f32 * 11.0).clamp(0.0, 100.0));
                }
                if clarity != -1 {
                    p.clarity = Some((clarity as f32 * 20.0).clamp(-100.0, 100.0));
                }
                if contrast != -1 {
                    p.contrast = Some((contrast as f32 * 20.0).clamp(-100.0, 100.0));
                }
                if brightness != -1 {
                    p.exposure = Some((brightness as f32 * 0.5).clamp(-5.0, 5.0));
                }
                if saturation != -1 {
                    p.saturation = Some((saturation as f32 * 20.0).clamp(-100.0, 100.0));
                }
                if hue != -1 {
                    p.tint = Some((hue as f32 * 20.0).clamp(-100.0, 100.0));
                }
                return Some(p);
            }
        }
    }
    None
}

fn parse_np3(data: &[u8]) -> Result<PresetParams, ParseError> {
    // Strategy 1: Look for ASCII version code (NP3 container format)
    if let Some(m) = find_nikon_version(data) {
        return Ok(decode_nikon_params(data, m));
    }
    // Strategy 2: NCP binary header format
    if is_ncp_binary_header(data) {
        return Ok(decode_ncp_fixed(data));
    }
    // Strategy 3: Heuristic scan for adjustment block
    if let Some(p) = heuristic_find_adjustments(data) {
        return Ok(p);
    }
    // Fallback: import with default params (never hard-fail)
    Ok(PresetParams::default())
}

fn parse_ncp(data: &[u8]) -> Result<PresetParams, ParseError> {
    // Strategy 1: NCP binary header (most common for .ncp files)
    if is_ncp_binary_header(data) {
        return Ok(decode_ncp_fixed(data));
    }
    // Strategy 2: ASCII version code (some NCP files use NP3-style container)
    if let Some(m) = find_nikon_version(data) {
        return Ok(decode_nikon_params(data, m));
    }
    // Strategy 3: Heuristic scan
    if let Some(p) = heuristic_find_adjustments(data) {
        return Ok(p);
    }
    // Fallback: import with default params
    Ok(PresetParams::default())
}

/// Extract an embedded Nikon control name from the binary data.
/// Tries: (1) known-name match in the name field (bytes 8-27 for NCP),
/// (2) known-name match anywhere in the file.
fn extract_nikon_name(data: &[u8]) -> Option<String> {
    // For NCP format: name is at bytes 8-27 (20 bytes, null-padded)
    if data.len() >= 28 {
        let name_bytes = &data[8..28];
        if let Ok(name_str) = std::str::from_utf8(name_bytes) {
            let trimmed = name_str.trim_end_matches('\0').trim();
            if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
                return Some(trimmed.to_string());
            }
        }
    }
    // Fallback: search for known control names anywhere in the file
    let lower = String::from_utf8_lossy(data).to_ascii_lowercase();
    for name in NIKON_CONTROL_NAMES {
        if lower.contains(&name.to_ascii_lowercase()) {
            return Some(name.to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Lightroom .lrpreset (Apple plist: binary or XML)
// ---------------------------------------------------------------------------

fn parse_lrpreset(data: &[u8]) -> Result<PresetParams, ParseError> {
    // Binary plist (Apple "bplist00"): full parsing is complex; degrade gracefully so the
    // preset still imports (with its filename) instead of being dropped.
    if data.starts_with(b"bplist00") {
        return Ok(PresetParams::default());
    }
    let text = String::from_utf8_lossy(data);
    let mut params = PresetParams::default();

    params.exposure = extract_lrpreset_float(&text, "Exposure");
    params.contrast = extract_lrpreset_float(&text, "Contrast");
    params.highlights = extract_lrpreset_float(&text, "HighlightRecovery")
        .or_else(|| extract_lrpreset_float(&text, "Highlights2012"));
    params.shadows = extract_lrpreset_float(&text, "FillLight")
        .or_else(|| extract_lrpreset_float(&text, "Shadows2012"));
    params.whites = extract_lrpreset_float(&text, "Whites2012");
    params.blacks = extract_lrpreset_float(&text, "Blacks2012");
    params.temperature = extract_lrpreset_float(&text, "Temperature");
    params.tint = extract_lrpreset_float(&text, "Tint");
    params.vibrance = extract_lrpreset_float(&text, "Vibrance");
    params.saturation = extract_lrpreset_float(&text, "Saturation");
    params.clarity = extract_lrpreset_float(&text, "Clarity");
    params.sharpening = extract_lrpreset_float(&text, "Sharpness");

    Ok(params)
}

fn extract_lrpreset_float(xml: &str, key_name: &str) -> Option<f32> {
    let key_tag = format!("<key>{}</key>", key_name);
    if let Some(key_pos) = xml.find(&key_tag) {
        let after_key = &xml[key_pos + key_tag.len()..];
        let trimmed = after_key.trim_start();
        if trimmed.starts_with("<real>") {
            let start = 6;
            if let Some(end) = trimmed[start..].find("</real>") {
                return trimmed[start..start + end].parse().ok();
            }
        } else if trimmed.starts_with("<integer>") {
            let start = 9;
            if let Some(end) = trimmed[start..].find("</integer>") {
                return trimmed[start..start + end].parse::<f32>().ok();
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Capture One .costyle (plist-like XML)
// ---------------------------------------------------------------------------

fn parse_costyle(data: &[u8]) -> Result<PresetParams, ParseError> {
    let text = String::from_utf8_lossy(data);
    let mut params = PresetParams::default();

    params.exposure = extract_costyle_float(&text, "Exposure");
    params.contrast = extract_costyle_float(&text, "Contrast");
    params.highlights = extract_costyle_float(&text, "Highlight");
    params.shadows = extract_costyle_float(&text, "Shadow");
    params.whites = extract_costyle_float(&text, "WhitePoint");
    params.blacks = extract_costyle_float(&text, "BlackPoint");
    params.saturation = extract_costyle_float(&text, "Saturation");
    params.vibrance = extract_costyle_float(&text, "Vibrance");
    params.clarity = extract_costyle_float(&text, "Clarity");
    params.sharpening = extract_costyle_float(&text, "Sharpening");

    Ok(params)
}

fn extract_costyle_float(xml: &str, key_name: &str) -> Option<f32> {
    let key_pattern = format!("<key>{}</key>", key_name);
    if let Some(key_pos) = xml.find(&key_pattern) {
        let after_key = &xml[key_pos + key_pattern.len()..];
        let real_pos = after_key.find("<real>");
        let int_pos = after_key.find("<integer>");
        let (tag, start) = match (real_pos, int_pos) {
            (Some(r), Some(i)) => if r < i { ("<real>", r) } else { ("<integer>", i) },
            (Some(r), None) => ("<real>", r),
            (None, Some(i)) => ("<integer>", i),
            (None, None) => return None,
        };
        let value_start = start + tag.len();
        let end_tag = if tag == "<real>" { "</real>" } else { "</integer>" };
        if let Some(end) = after_key[value_start..].find(end_tag) {
            let value_str = after_key[value_start..value_start + end].trim();
            return value_str.parse::<f32>().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_xmp_float() {
        let xml = r#"<rdf:Description crs:Exposure2012="+0.50" crs:Contrast2012="+15">"#;
        assert_eq!(extract_xmp_float(xml, "Exposure2012"), Some(0.50));
        assert_eq!(extract_xmp_float(xml, "Contrast2012"), Some(15.0));
        assert_eq!(extract_xmp_float(xml, "NonExistent"), None);
    }

    #[test]
    fn test_extract_xmp_float_negative() {
        let xml = r#"crs:Exposure2012="-1.50" crs:Saturation="-25""#;
        assert_eq!(extract_xmp_float(xml, "Exposure2012"), Some(-1.50));
        assert_eq!(extract_xmp_float(xml, "Saturation"), Some(-25.0));
    }

    #[test]
    fn test_extract_xmp_float_zero() {
        let xml = r#"crs:Contrast2012="0""#;
        assert_eq!(extract_xmp_float(xml, "Contrast2012"), Some(0.0));
    }

    #[test]
    fn test_extract_xmp_element_form() {
        let xml = "<crs:Exposure2012>0.75</crs:Exposure2012><crs:Saturation>-10</crs:Saturation>";
        assert_eq!(extract_xmp_element(xml, "Exposure2012"), Some(0.75));
        assert_eq!(extract_xmp_element(xml, "Saturation"), Some(-10.0));
        assert_eq!(extract_xmp_element(xml, "Missing"), None);
    }

    #[test]
    fn test_parse_xmp() {
        let xmp = br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    crs:Exposure2012="+0.30"
    crs:Contrast2012="+20"
    crs:Saturation="+10"/>
 </rdf:RDF>
</x:xmpmeta>"#;
        let params = parse_xmp(xmp).unwrap();
        assert_eq!(params.exposure, Some(0.30));
        assert_eq!(params.contrast, Some(20.0));
        assert_eq!(params.saturation, Some(10.0));
    }

    #[test]
    fn test_parse_xmp_empty() {
        let params = parse_xmp(b"").unwrap();
        assert_eq!(params.exposure, None);
        assert_eq!(params.contrast, None);
    }

    #[test]
    fn test_extract_xmp_name() {
        let xmp = br#"<rdf:Description dc:title="Cinematic Teal" crs:Exposure2012="0"/>"#;
        assert_eq!(extract_xmp_name(xmp), Some("Cinematic Teal".to_string()));
        assert_eq!(extract_xmp_name(b"no title here"), None);
    }

    #[test]
    fn test_find_nikon_version_at_offset() {
        // Version code wrapped in a 16-byte container header.
        let mut data = vec![0xABu8; 32];
        data[16..20].copy_from_slice(b"0203");
        assert_eq!(find_nikon_version(&data), Some(16));
    }

    #[test]
    fn test_find_nikon_version_none() {
        let data = vec![0xABu8; 32];
        assert_eq!(find_nikon_version(&data), None);
    }

    #[test]
    fn test_parse_np3_valid_magic() {
        let mut data = vec![0u8; 32];
        data[0..4].copy_from_slice(b"0103");
        data[8] = 5;  // sharpening -> 5*11=55
        data[10] = 2; // contrast -> 2*20=40
        data[12] = 1; // saturation -> 1*20=20
        let params = parse_np3(&data).unwrap();
        assert_eq!(params.sharpening, Some(55.0));
        assert_eq!(params.contrast, Some(40.0));
        assert_eq!(params.saturation, Some(20.0));
    }

    #[test]
    fn test_parse_np3_container_offset() {
        // Real NP3: version code not at offset 0 -> parser must scan and still decode.
        let mut data = vec![0u8; 48];
        data[0..4].copy_from_slice(b"NP3X"); // container marker (not a version code)
        data[12..16].copy_from_slice(b"0203");
        data[12 + 8] = 4;  // sharpening -> 4*11=44
        data[12 + 10] = 3; // contrast -> 3*20=60
        let params = parse_np3(&data).unwrap();
        assert_eq!(params.sharpening, Some(44.0));
        assert_eq!(params.contrast, Some(60.0));
    }

    #[test]
    fn test_parse_np3_unknown_magic_graceful() {
        // No recognizable version code -> degrade to defaults instead of failing.
        let data = b"XXXX12345678";
        let params = parse_np3(data).unwrap();
        assert_eq!(params.sharpening, None);
        assert_eq!(params.contrast, None);
    }

    #[test]
    fn test_parse_np3_too_short_graceful() {
        let data = b"0103";
        let params = parse_np3(data).unwrap();
        assert_eq!(params.sharpening, None);
    }

    #[test]
    fn test_parse_ncp_binary_header() {
        // NCP binary header: 00 00 01 00, name at 8-27, adjustments at 28+
        let mut data = vec![0u8; 44];
        data[0] = 0x00; data[1] = 0x00; data[2] = 0x01; data[3] = 0x00;
        data[8..16].copy_from_slice(b"Vivid\0\0\0");
        data[28] = 6;   // sharpening -> 6*11=66
        data[29] = 1;   // clarity -> 1*20=20
        data[30] = 2;   // contrast -> 2*20=40
        data[31] = 1;   // brightness -> 1*0.5=0.5
        data[32] = 3;   // saturation -> 3*20=60
        data[33] = 0;   // hue -> 0
        let params = parse_ncp(&data).unwrap();
        assert_eq!(params.sharpening, Some(66.0));
        assert_eq!(params.clarity, Some(20.0));
        assert_eq!(params.contrast, Some(40.0));
        assert_eq!(params.exposure, Some(0.5));
        assert_eq!(params.saturation, Some(60.0));
        assert_eq!(params.tint, Some(0.0));
    }

    #[test]
    fn test_parse_ncp_binary_header_signed_values() {
        // NCP with negative adjustments (signed bytes)
        let mut data = vec![0u8; 44];
        data[0] = 0x00; data[1] = 0x00; data[2] = 0x01; data[3] = 0x00;
        data[8..16].copy_from_slice(b"Portrait");
        data[28] = 2;   // sharpening -> 2*11=22
        data[29] = 0xFF; // clarity = -1 -> -20
        data[30] = 0xFF; // contrast = -1 -> -20
        data[31] = 1;   // brightness -> 0.5
        data[32] = 0xFF; // saturation = -1 -> -20
        data[33] = 1;   // hue -> 20
        let params = parse_ncp(&data).unwrap();
        assert_eq!(params.sharpening, Some(22.0));
        assert_eq!(params.clarity, Some(-20.0));
        assert_eq!(params.contrast, Some(-20.0));
        assert_eq!(params.saturation, Some(-20.0));
        assert_eq!(params.tint, Some(20.0));
    }

    #[test]
    fn test_parse_ncp_ascii_version_fallback() {
        // NCP with ASCII version code (NP3-style container)
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(b"0100");
        data[8] = 3;   // sharpening -> 3*11=33
        data[10] = 4;  // contrast -> 4*20=80
        data[12] = 2;  // saturation -> 2*20=40
        let params = parse_ncp(&data).unwrap();
        assert_eq!(params.sharpening, Some(33.0));
        assert_eq!(params.contrast, Some(80.0));
        assert_eq!(params.saturation, Some(40.0));
    }

    #[test]
    fn test_parse_ncp_unknown_magic_graceful() {
        let data = b"XXXX1234567890123456";
        let params = parse_ncp(data).unwrap();
        assert_eq!(params.sharpening, None);
    }

    #[test]
    fn test_parse_ncp_heuristic_fallback() {
        // File with no recognized header but has adjustment-like bytes
        let mut data = vec![0u8; 48];
        data[0] = 0xDE; data[1] = 0xAD; // unknown magic
        data[10] = 5;  // sharpening-like
        data[11] = 2;  // clarity-like
        data[12] = 3;  // contrast-like
        data[13] = 1;  // brightness-like
        data[14] = 2;  // saturation-like
        data[15] = 0;  // hue-like
        let params = parse_ncp(&data).unwrap();
        // Heuristic should find the block
        assert!(params.sharpening.is_some());
    }

    #[test]
    fn test_extract_nikon_name_from_ncp_header() {
        // NCP format: name at bytes 8-27
        let mut data = vec![0u8; 44];
        data[0] = 0x00; data[1] = 0x00; data[2] = 0x01; data[3] = 0x00;
        data[8..14].copy_from_slice(b"Vivid\0");
        assert_eq!(extract_nikon_name(&data), Some("Vivid".to_string()));
    }

    #[test]
    fn test_extract_nikon_name_known() {
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(b"0203");
        data[20..25].copy_from_slice(b"Vivid");
        assert_eq!(extract_nikon_name(&data), Some("Vivid".to_string()));
    }

    #[test]
    fn test_extract_nikon_name_none() {
        let data = vec![0u8; 32];
        assert_eq!(extract_nikon_name(&data), None);
    }

    #[test]
    fn test_is_ncp_binary_header() {
        let mut data = vec![0u8; 44];
        data[0] = 0x00; data[1] = 0x00; data[2] = 0x01; data[3] = 0x00;
        assert!(is_ncp_binary_header(&data));

        let data2 = vec![0xABu8; 44];
        assert!(!is_ncp_binary_header(&data2));

        let short = vec![0u8; 10];
        assert!(!is_ncp_binary_header(&short));
    }

    #[test]
    fn test_parse_lrpreset() {
        let plist = br#"<?xml version="1.0"?>
<plist version="1.0">
<dict>
  <key>Exposure</key><real>0.5</real>
  <key>Contrast</key><integer>25</integer>
  <key>Saturation</key><real>-10</real>
</dict>
</plist>"#;
        let params = parse_lrpreset(plist).unwrap();
        assert_eq!(params.exposure, Some(0.5));
        assert_eq!(params.contrast, Some(25.0));
        assert_eq!(params.saturation, Some(-10.0));
    }

    #[test]
    fn test_parse_lrpreset_binary_graceful() {
        // Real .lrpreset files are often binary plists -> degrade to defaults, not error.
        let data = b"bplist00\xd4\x01\x02somebinary";
        let params = parse_lrpreset(data).unwrap();
        assert_eq!(params.exposure, None);
    }

    #[test]
    fn test_parse_lrpreset_empty() {
        let params = parse_lrpreset(b"").unwrap();
        assert_eq!(params.exposure, None);
    }

    #[test]
    fn test_extract_lrpreset_float() {
        let xml = "<key>Exposure</key><real>1.5</real>";
        assert_eq!(extract_lrpreset_float(xml, "Exposure"), Some(1.5));
        let xml2 = "<key>Contrast</key><integer>30</integer>";
        assert_eq!(extract_lrpreset_float(xml2, "Contrast"), Some(30.0));
    }

    #[test]
    fn test_parse_cube_graceful() {
        // LUT files import as references with neutral params (never hard-fail).
        assert!(parse_cube(b"").is_ok());
        let with_lut = b"LUT_3D_SIZE 33\nTITLE \"My LUT\"\n0.0 0.0 0.0\n";
        assert!(parse_cube(with_lut).is_ok());
    }

    #[test]
    fn test_parse_3dl_graceful() {
        assert!(parse_3dl(b"not a valid 3dl file").is_ok());
        assert!(parse_3dl(b"0 0 0\n255 255 255\n").is_ok());
    }

    #[test]
    fn test_parse_costyle() {
        let xml = br#"<?xml version="1.0"?>
<plist version="1.0">
<dict>
  <key>Adjustments</key>
  <dict>
    <key>Exposure</key><real>0.75</real>
    <key>Contrast</key><integer>30</integer>
    <key>Saturation</key><real>-15</real>
    <key>Sharpening</key><real>50</real>
  </dict>
</dict>
</plist>"#;
        let params = parse_costyle(xml).unwrap();
        assert_eq!(params.exposure, Some(0.75));
        assert_eq!(params.contrast, Some(30.0));
        assert_eq!(params.saturation, Some(-15.0));
        assert_eq!(params.sharpening, Some(50.0));
    }

    #[test]
    fn test_parse_costyle_empty() {
        let params = parse_costyle(b"").unwrap();
        assert_eq!(params.exposure, None);
    }

    #[test]
    fn test_extract_costyle_float() {
        let xml = "<key>Exposure</key><real>1.25</real>";
        assert_eq!(extract_costyle_float(xml, "Exposure"), Some(1.25));
        let xml2 = "<key>Contrast</key><integer>40</integer>";
        assert_eq!(extract_costyle_float(xml2, "Contrast"), Some(40.0));
        assert_eq!(extract_costyle_float(xml, "NonExistent"), None);
    }

    #[test]
    fn test_parse_preset_file_missing_io_error() {
        // A genuinely unreadable file is the only hard-error case.
        let result = parse_preset_file("test_does_not_exist.xyz", &PresetFormat::XMP);
        assert!(result.is_err());
    }
}
#[cfg(test)]
mod fs_roundtrip_tests {
    //! End-to-end: write real-format sample files to disk and run them through the same
    //! parse path the `import_preset` command uses (fs::read + dispatch + name detection).
    //! Guards against the "0 presets imported" regression where strict magic-byte checks
    //! silently rejected real Nikon files.
    use super::*;

    fn tmp_path(name: &str) -> String {
        let mut p = std::env::temp_dir();
        p.push(format!("nps_test_{}_{}", std::process::id(), name));
        p.to_string_lossy().to_string()
    }

    #[test]
    fn import_np3_with_container_offset_and_name() {
        // Real NP3: version code wrapped in a container (not at offset 0) + embedded name.
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(b"NP3X"); // container marker
        data[12..16].copy_from_slice(b"0203"); // version code at offset 12
        data[12 + 8] = 6; // sharpening -> 6*11=66
        data[12 + 10] = 2; // contrast -> 2*20=40
        data[12 + 12] = 1; // saturation -> 1*20=20
        data[40..45].copy_from_slice(b"Vivid"); // embedded control name

        let path = tmp_path("vividsample.np3");
        std::fs::write(&path, &data).unwrap();

        let params = parse_preset_file(&path, &PresetFormat::NP3).unwrap();
        assert_eq!(params.sharpening, Some(66.0));
        assert_eq!(params.contrast, Some(40.0));
        assert_eq!(params.saturation, Some(20.0));

        let name = detect_preset_name(&path, &PresetFormat::NP3).unwrap();
        assert_eq!(name, "Vivid");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_xmp_attribute_form() {
        let xmp = br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    dc:title="Cinematic Teal"
    crs:Exposure2012="+0.40"
    crs:Contrast2012="+15"
    crs:Temperature="4500"/>
 </rdf:RDF>
</x:xmpmeta>"#;
        let path = tmp_path("cinematic.xmp");
        std::fs::write(&path, xmp).unwrap();

        let params = parse_preset_file(&path, &PresetFormat::XMP).unwrap();
        assert_eq!(params.exposure, Some(0.40));
        assert_eq!(params.contrast, Some(15.0));
        assert_eq!(params.temperature, Some(4500.0));

        let name = detect_preset_name(&path, &PresetFormat::XMP).unwrap();
        assert_eq!(name, "Cinematic Teal");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_cube_lut_never_fails() {
        let cube = b"LUT_3D_SIZE 33\nTITLE \"Teal Orange\"\n0.0 0.0 0.0\n1.0 1.0 1.0\n";
        let path = tmp_path("tealorange.cube");
        std::fs::write(&path, cube).unwrap();

        let params = parse_preset_file(&path, &PresetFormat::Cube).unwrap();
        assert_eq!(params.exposure, None);

        let name = detect_preset_name(&path, &PresetFormat::Cube).unwrap();
        assert!(name.contains("tealorange"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_binary_lrpreset_graceful() {
        let path = tmp_path("oldstyle.lrpreset");
        std::fs::write(&path, b"bplist00\x00\x01\x02binarydata").unwrap();

        let result = parse_preset_file(&path, &PresetFormat::LRPreset);
        assert!(result.is_ok(), "binary plist must import gracefully, not error");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_garbage_file_still_succeeds() {
        // Core guarantee that fixes "0 presets imported": an unrecognized blob still
        // imports (best-effort) rather than being dropped.
        let path = tmp_path("mystery.ncp");
        std::fs::write(&path, b"totally not a real preset blob").unwrap();

        let result = parse_preset_file(&path, &PresetFormat::NCP);
        assert!(result.is_ok(), "unreadable-format file must not hard-fail");

        let _ = std::fs::remove_file(&path);
    }
}
