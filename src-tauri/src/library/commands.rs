use serde::Serialize;
use super::{ExifData, PhotoRecord};

#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub folder: String,
    pub count: usize,
    pub files: Vec<String>,
}

/// A photo discovered during import, with enough metadata for the UI grid.
#[derive(Debug, Clone, Serialize)]
pub struct ImportedPhoto {
    pub file_path: String,
    pub filename: String,
    pub format: String,
    pub file_size: u64,
    pub width: u32,
    pub height: u32,
    /// true when a thumbnail can be produced (raster directly, or NEF via its
    /// embedded JPEG preview).
    pub previewable: bool,
}

#[tauri::command]
pub async fn scan_folder(path: String) -> Result<ScanResult, String> {
    let files = super::scan_directory(&path);
    let count = files.len();
    Ok(ScanResult { folder: path, count, files })
}

/// Recursively import every supported image under `path`.
#[tauri::command]
pub async fn import_folder(path: String) -> Result<Vec<ImportedPhoto>, String> {
    let mut out: Vec<ImportedPhoto> = Vec::new();
    collect_images(std::path::Path::new(&path), &mut out);
    out.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(out)
}

fn collect_images(dir: &std::path::Path, out: &mut Vec<ImportedPhoto>) {
    let raster = ["jpg", "jpeg", "png", "tif", "tiff"];
    let raw = ["nef"];
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_images(&p, out);
            continue;
        }
        let ext = match p.extension().and_then(|e| e.to_str()) {
            Some(e) => e.to_lowercase(),
            None => continue,
        };
        let is_raster = raster.contains(&ext.as_str());
        let is_raw = raw.contains(&ext.as_str());
        if !is_raster && !is_raw {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let (w, h) = if is_raster {
            image::image_dimensions(&p).unwrap_or((0, 0))
        } else {
            (0, 0)
        };
        out.push(ImportedPhoto {
            filename: p.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string(),
            file_path: p.to_string_lossy().to_string(),
            format: ext.to_uppercase(),
            file_size: size,
            width: w,
            height: h,
            previewable: true,
        });
    }
}

/// Dimensions of an in-memory JPEG (used for NEF embedded previews).
fn jpeg_dimensions(j: &[u8]) -> Option<(u32, u32)> {
    image::load_from_memory(j).ok().map(|i| (i.width(), i.height()))
}

/// Read EXIF orientation tag (0x0112) from JPEG data.
/// Returns orientation value 1-8, defaults to 1 (normal).
fn jpeg_exif_orientation(data: &[u8]) -> u8 {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return 1;
    }
    let mut pos = 2usize;
    while pos + 4 < data.len() {
        if data[pos] != 0xFF { pos += 1; continue; }
        let marker = data[pos + 1];
        if marker == 0xE1 {
            // APP1
            if pos + 4 >= data.len() { break; }
            let seg_len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
            let seg_start = pos + 4;
            if seg_start + 6 <= data.len() && &data[seg_start..seg_start + 6] == b"Exif\0\0" {
                let tiff_start = seg_start + 6;
                if tiff_start < data.len() {
                    return read_tiff_orientation(&data[tiff_start..]);
                }
            }
            pos += 2 + seg_len;
        } else if marker == 0xDA {
            break; // Start of Scan - stop
        } else if marker == 0xD8 || marker == 0xD9 || (marker >= 0xD0 && marker <= 0xD7) {
            pos += 2;
        } else {
            if pos + 4 >= data.len() { break; }
            let seg_len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
            pos += 2 + seg_len;
        }
    }
    1
}

/// Read orientation tag from TIFF-structured data (JPEG EXIF or NEF header).
fn read_tiff_orientation(tiff: &[u8]) -> u8 {
    if tiff.len() < 8 { return 1; }
    let le = match &tiff[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return 1,
    };
    let r16 = |off: usize| -> u16 {
        if off + 2 > tiff.len() { return 0; }
        if le { u16::from_le_bytes([tiff[off], tiff[off+1]]) }
        else { u16::from_be_bytes([tiff[off], tiff[off+1]]) }
    };
    let r32 = |off: usize| -> u32 {
        if off + 4 > tiff.len() { return 0; }
        if le { u32::from_le_bytes([tiff[off], tiff[off+1], tiff[off+2], tiff[off+3]]) }
        else { u32::from_be_bytes([tiff[off], tiff[off+1], tiff[off+2], tiff[off+3]]) }
    };
    let ifd_off = r32(4) as usize;
    if ifd_off + 2 > tiff.len() { return 1; }
    let count = r16(ifd_off) as usize;
    for i in 0..count {
        let e = ifd_off + 2 + i * 12;
        if e + 12 > tiff.len() { break; }
        if r16(e) == 0x0112 {
            let v = r16(e + 8);
            return if (1..=8).contains(&v) { v as u8 } else { 1 };
        }
    }
    1
}

/// Apply EXIF orientation transform so the image displays correctly.
fn apply_orientation(img: image::DynamicImage, orientation: u8) -> image::DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// On-disk render cache.
///
/// The single biggest loading speed-up: generating a thumbnail used to re-read
/// the whole NEF (often 20-30 MB), re-parse its TIFF structure, decode the
/// embedded preview, resize and re-encode it on *every* request. Now the
/// rendered JPEG (and the extracted NEF preview) are cached on disk keyed by
/// (path, mtime, size, render params), so repeat requests — including after an
/// app restart — are served by reading a tiny ~30 KB file instead of the RAW.
mod thumb_cache {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::path::PathBuf;

    fn cache_dir() -> Option<PathBuf> {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .ok()?;
        Some(
            std::path::Path::new(&home)
                .join(".nikon-photo-studio")
                .join("thumb_cache"),
        )
    }

    /// Resolve the cache file for a source image + render params, or `None` if
    /// the source can't be stat'd or no cache dir is available. The key folds
    /// in mtime + size so editing a file invalidates its stale entries.
    pub fn entry_path(source: &str, max_size: u32, kind: &str) -> Option<PathBuf> {
        let meta = std::fs::metadata(source).ok()?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let size = meta.len();
        let mut h = DefaultHasher::new();
        source.hash(&mut h);
        mtime.hash(&mut h);
        size.hash(&mut h);
        max_size.hash(&mut h);
        kind.hash(&mut h);
        let dir = cache_dir()?;
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir.join(format!("{:016x}.{}", h.finish(), kind)))
    }

    pub fn read(entry: &std::path::Path) -> Option<Vec<u8>> {
        std::fs::read(entry).ok()
    }

    /// Persist bytes to a cache entry atomically: write to a unique sibling
    /// temp file, then rename over the destination. This never hands a reader a
    /// half-written file and sidesteps most transient Windows sharing-violations
    /// on the destination. Returns whether the file now exists on disk.
    pub fn write(entry: &std::path::Path, bytes: &[u8]) -> bool {
        let dir = match entry.parent() {
            Some(d) => d,
            None => return false,
        };
        if std::fs::create_dir_all(dir).is_err() {
            return false;
        }
        let nonce = (std::process::id() as u128) * 1_000_000
            + std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as u128)
                .unwrap_or(0);
        let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("x");
        let tmp = dir.join(format!(".tmp-{:016x}-{}", nonce, name));
        if std::fs::write(&tmp, bytes).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return false;
        }
        if std::fs::rename(&tmp, entry).is_ok() {
            return true;
        }
        // Rename blocked (e.g. momentary lock): fall back to a direct write.
        let _ = std::fs::remove_file(&tmp);
        std::fs::write(entry, bytes).is_ok()
    }
}

/// On-disk cache of analysis-ready grayscale buffers for blur detection.
///
/// Decoding the NEF preview JPEG is the dominant per-file cost (the pure-Rust
/// JPEG decoder is especially slow in unoptimized/debug builds, ~100ms even for
/// a 0.4 MP preview). Blur detection only needs a small grayscale buffer, so
/// once we have decoded + oriented + resized it we cache the raw 8-bit pixels
/// keyed by (path, mtime, size, analysis dim). Re-running detection (e.g. when
/// the user tunes the sensitivity slider) then reads a few hundred KB from disk
/// and skips both the large RAW read and the JPEG decode entirely.
///
/// The gray values are integer 0-255, so the u8 round-trip is lossless and the
/// resulting blur score is bit-identical to a fresh decode.
pub(crate) fn analysis_cache_read(path: &str, max_dim: u32) -> Option<(Vec<f32>, usize, usize)> {
    let entry = thumb_cache::entry_path(path, max_dim, "blurprev")?;
    let bytes = thumb_cache::read(&entry)?;
    if bytes.len() < 8 {
        return None;
    }
    let w = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let h = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if w == 0 || h == 0 || w.saturating_mul(h) != bytes.len() - 8 {
        return None;
    }
    let gray = bytes[8..].iter().map(|&p| p as f32).collect();
    Some((gray, w, h))
}

/// Persist an analysis grayscale buffer to the on-disk cache. Best-effort: a
/// failed write simply means the next run re-decodes (correctness unaffected).
pub(crate) fn analysis_cache_write(path: &str, max_dim: u32, gray: &[f32], w: usize, h: usize) {
    if let Some(entry) = thumb_cache::entry_path(path, max_dim, "blurprev") {
        let mut bytes = Vec::with_capacity(8 + gray.len());
        bytes.extend_from_slice(&(w as u32).to_le_bytes());
        bytes.extend_from_slice(&(h as u32).to_le_bytes());
        bytes.extend(gray.iter().map(|&v| v.round().clamp(0.0, 255.0) as u8));
        thumb_cache::write(&entry, &bytes);
    }
}

/// Encode a `DynamicImage` to JPEG bytes at the given quality.
fn encode_jpeg(img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let mut buf: Vec<u8> = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    let rgb = img.to_rgb8();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, quality);
    encoder
        .write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
        .map_err(|e| format!("encode failed: {e}"))?;
    Ok(buf)
}

/// Decode → orient → resize → encode. The slow path used on a cache miss.
fn render_resized(
    path: &str,
    max: u32,
    filter: image::imageops::FilterType,
    quality: u8,
) -> Result<Vec<u8>, String> {
    let img = load_oriented_image(path)?;
    let out = if img.width() <= max && img.height() <= max {
        img
    } else {
        img.resize(max, max, filter)
    };
    encode_jpeg(&out, quality)
}

/// Produce a develop-viewer preview for an ordinary raster file. A JPEG that
/// already fits the requested size and has normal EXIF orientation is returned
/// byte-for-byte: decoding and JPEG-encoding it again softens fine detail that
/// becomes visible at 400-800% zoom. Files that need resizing/orientation (and
/// non-JPEG formats) still use the high-quality render path.
fn render_raster_full_preview(path: &str, max: u32) -> Result<Vec<u8>, String> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext == "jpg" || ext == "jpeg" {
        let bytes = std::fs::read(path).map_err(|e| format!("read failed: {e}"))?;
        let orientation = jpeg_exif_orientation(&bytes);
        if orientation == 1 {
            if let Some((w, h)) = jpeg_dimensions(&bytes) {
                if w <= max && h <= max {
                    return Ok(bytes);
                }
            }
        }
    }
    render_resized(path, max, image::imageops::FilterType::Lanczos3, 98)
}

fn data_url(bytes: &[u8]) -> String {
    format!("data:image/jpeg;base64,{}", base64_encode::encode(bytes))
}

/// Read a NEF's embedded preview JPEG + orientation WITHOUT reading the whole
/// RAW file. We read only a ~1 MiB "head" (all the TIFF/IFD metadata lives
/// there), locate the preview JPEG via find_preview_location, then seek
/// straight to that offset and read just the JPEG (~1 MB) instead of the full
/// 20-30 MB file. On a cold import of hundreds of NEFs this cuts disk I/O by
/// roughly an order of magnitude. Falls back to a full read + extract_preview
/// for unusual layouts the head parse can't resolve.
fn read_nef_preview_partial(path: &str) -> Result<(Vec<u8>, u8), String> {
    use std::io::{Read, Seek, SeekFrom};
    const HEAD: usize = 1 << 20; // 1 MiB metadata head (Nikon IFDs fit easily)

    let mut f = std::fs::File::open(path).map_err(|e| format!("open failed: {e}"))?;
    let file_size = f
        .metadata()
        .map(|m| m.len() as usize)
        .map_err(|e| format!("stat failed: {e}"))?;
    let head_len = file_size.min(HEAD);
    let mut head = vec![0u8; head_len];
    f.read_exact(&mut head)
        .map_err(|e| format!("read head failed: {e}"))?;
    let orientation = read_tiff_orientation(&head);

    if let Some((off, len)) = nef::find_preview_location(&head, file_size) {
        // Fast path A: the JPEG already lies inside the head buffer.
        if off.checked_add(len).map(|e| e <= head_len).unwrap_or(false) {
            let jpeg = &head[off..off + len];
            if nef::is_jpeg(jpeg) {
                return Ok((jpeg.to_vec(), orientation));
            }
        }
        // Fast path B: seek to the JPEG (beyond the head) and read just it.
        if off.checked_add(len).map(|e| e <= file_size).unwrap_or(false)
            && f.seek(SeekFrom::Start(off as u64)).is_ok()
        {
            let mut jpeg = vec![0u8; len];
            if f.read_exact(&mut jpeg).is_ok() && nef::is_jpeg(&jpeg) {
                return Ok((jpeg, orientation));
            }
        }
    }

    // Fallback: full read + robust extractor for unusual layouts.
    drop(f);
    let raw = std::fs::read(path).map_err(|e| format!("read failed: {e}"))?;
    let orientation = read_tiff_orientation(&raw);
    let jpeg = nef::extract_preview(&raw).ok_or_else(|| "no embedded preview".to_string())?;
    Ok((jpeg, orientation))
}

/// Return the NEF's embedded preview JPEG bytes + orientation, using a disk
/// cache so the large RAW file is read & parsed only once per file version.
/// On a cache miss we use the partial reader (head parse + seek) so even the
/// first access avoids reading the entire RAW file.
fn nef_preview_cached(path: &str) -> Result<(Vec<u8>, u8), String> {
    if let Some(entry) = thumb_cache::entry_path(path, 0, "nefprev") {
        if let Some(bytes) = thumb_cache::read(&entry) {
            if let Some((&ori, jpeg)) = bytes.split_first() {
                if !jpeg.is_empty() {
                    return Ok((jpeg.to_vec(), ori));
                }
            }
        }
        let (jpeg, orientation) = read_nef_preview_partial(path)?;
        let mut combined = Vec::with_capacity(jpeg.len() + 1);
        combined.push(orientation);
        combined.extend_from_slice(&jpeg);
        thumb_cache::write(&entry, &combined);
        return Ok((jpeg, orientation));
    }
    // No cache dir available: decode directly via the partial reader.
    read_nef_preview_partial(path)
}

/// Load an image file as a correctly-oriented `DynamicImage`.
///
/// NEF files are decoded through their (cached) embedded JPEG preview — no full
/// RAW demosaic and, on repeat access, no re-read of the large RAW file. Raster
/// files (JPEG/PNG/TIFF) are decoded directly. EXIF / TIFF orientation is
/// applied so portrait photos come out upright.
///
/// This is the single shared decode path used by thumbnail generation, the
/// develop full-preview, and blur analysis so they all stay consistent & fast.
pub fn load_oriented_image(path: &str) -> Result<image::DynamicImage, String> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext == "nef" {
        let (jpeg, orientation) = nef_preview_cached(path)?;
        let decoded = image::load_from_memory(&jpeg).map_err(|e| format!("decode preview failed: {e}"))?;
        Ok(apply_orientation(decoded, orientation))
    } else {
        let bytes = std::fs::read(path).map_err(|e| format!("read failed: {e}"))?;
        let orientation = if ext == "jpg" || ext == "jpeg" {
            jpeg_exif_orientation(&bytes)
        } else {
            1
        };
        let decoded = image::load_from_memory(&bytes).map_err(|e| format!("decode failed: {e}"))?;
        Ok(apply_orientation(decoded, orientation))
    }
}

/// Synchronous thumbnail render shared by the single-photo and batch commands.
///
/// Small thumbnails use a fast bilinear (Triangle) filter — visually identical
/// at icon sizes but many times faster than Lanczos3. Large (develop-viewer)
/// sizes keep Lanczos3 for maximum sharpness. Checks the on-disk cache first;
/// on a miss it decodes → orients → resizes → encodes → writes the cache and
/// returns a base64 data URL.
fn render_thumbnail_sync(path: &str, max: u32) -> Result<String, String> {
    let filter = if max >= 1000 {
        image::imageops::FilterType::Lanczos3
    } else {
        image::imageops::FilterType::Triangle
    };
    let quality = if max >= 2000 { 96u8 } else if max >= 1000 { 94u8 } else { 90u8 };

    if let Some(entry) = thumb_cache::entry_path(path, max, "thumb") {
        if let Some(bytes) = thumb_cache::read(&entry) {
            return Ok(data_url(&bytes));
        }
        let buf = render_resized(path, max, filter, quality)?;
        thumb_cache::write(&entry, &buf);
        return Ok(data_url(&buf));
    }
    let buf = render_resized(path, max, filter, quality)?;
    Ok(data_url(&buf))
}

/// Ensure a thumbnail JPEG exists in the on-disk cache and return its absolute
/// path. This backs the asset-protocol fast path: instead of base64-encoding
/// each JPEG into the IPC payload (~24 KB/photo, ~20 MB of JSON for an
/// 888-photo folder), the batch command returns just the file path and the
/// frontend serves it through Tauri's asset protocol (http://asset.localhost/),
/// which streams the bytes straight from disk and sniffs image/jpeg from the
/// JPEG magic bytes. The cache is content-addressed, so a hit is a no-op.
fn render_thumbnail_to_cache(path: &str, max: u32) -> Result<std::path::PathBuf, String> {
    let filter = if max >= 1000 {
        image::imageops::FilterType::Lanczos3
    } else {
        image::imageops::FilterType::Triangle
    };
    let quality = if max >= 2000 { 96u8 } else if max >= 1000 { 94u8 } else { 90u8 };

    let entry = thumb_cache::entry_path(path, max, "thumb")
        .ok_or_else(|| "no cache directory available".to_string())?;
    if entry.exists() {
        return Ok(entry);
    }
    let buf = render_resized(path, max, filter, quality)?;
    // Persist with a short retry loop: on Windows a freshly-created file can be
    // briefly locked by antivirus / the search indexer, so a single failed write
    // must not surface as a missing thumbnail. We only report success once the
    // file genuinely exists, otherwise the caller falls back to base64.
    for attempt in 0..6u32 {
        if thumb_cache::write(&entry, &buf) && entry.exists() {
            return Ok(entry);
        }
        if attempt < 5 {
            std::thread::sleep(std::time::Duration::from_millis(15 * (attempt + 1) as u64));
        }
    }
    Err(format!("failed to persist thumbnail cache file for {path}"))
}

/// Generate a JPEG thumbnail (base64 data URL).
/// Works for raster images directly and for NEF via the embedded JPEG preview.
/// Automatically applies EXIF orientation so portrait photos display correctly.
/// Rendered thumbnails are cached on disk; the blocking work runs on the
/// blocking thread pool so many thumbnails generate concurrently without
/// stalling the async runtime.
#[tauri::command]
pub async fn make_thumbnail(path: String, max_size: Option<u32>) -> Result<String, String> {
    let max = max_size.unwrap_or(320);
    tokio::task::spawn_blocking(move || render_thumbnail_sync(&path, max))
        .await
        .map_err(|e| format!("thumbnail task failed: {e}"))?
}

/// Return the pixel dimensions of a NEF's embedded preview (best effort).
#[tauri::command]
pub async fn nef_preview_size(path: String) -> Result<(u32, u32), String> {
    let (jpeg, _) = nef_preview_cached(&path)?;
    jpeg_dimensions(&jpeg).ok_or_else(|| "cannot read preview dimensions".to_string())
}

/// Map a `rawler` orientation enum to the standard TIFF/EXIF orientation code
/// (1-8) understood by `apply_orientation`.
fn rawler_orientation_to_u8(o: rawler::decoders::Orientation) -> u8 {
    use rawler::decoders::Orientation as O;
    match o {
        O::Normal | O::Unknown => 1,
        O::HorizontalFlip => 2,
        O::Rotate180 => 3,
        O::VerticalFlip => 4,
        O::Transpose => 5,
        O::Rotate90 => 6,
        O::Transverse => 7,
        O::Rotate270 => 8,
    }
}

/// Full-resolution NEF develop: decode the RAW sensor data, demosaic (PPG),
/// apply white balance + camera color matrix + sRGB gamma (rawler's default
/// pipeline), orient, resize to `max` and encode as JPEG.
///
/// This yields a far sharper centered image than the camera's small (~2 MP)
/// embedded preview JPEG, because it uses the sensor's native resolution. It is
/// heavier (full demosaic of a multi-megapixel RAW), so it is reserved for the
/// develop viewer's full preview and cached on disk (kind "rawfull").
fn develop_nef_full(path: &str) -> Result<image::DynamicImage, String> {
    let rawimage = rawler::decode_file(path).map_err(|e| format!("raw decode failed: {e}"))?;
    let rawler_orientation = rawler_orientation_to_u8(rawimage.orientation);
    let dev = rawler::imgop::develop::RawDevelop::default();
    let intermediate = dev
        .develop_intermediate(&rawimage)
        .map_err(|e| format!("raw develop failed: {e}"))?;
    // Release the large RAW buffer before materialising the RGB image so peak
    // memory stays bounded for high-megapixel bodies (e.g. Z5II ~45 MP).
    drop(rawimage);
    let img = intermediate
        .to_dynamic_image()
        .ok_or_else(|| "raw develop produced no image".to_string())?;
    // Nikon's embedded preview is the ground truth for presentation
    // orientation. Some rawler decoders already return pixels with width and
    // height swapped for portrait captures; blindly applying the metadata a
    // second time turns an upright portrait sideways. Compare against the
    // correctly-oriented embedded preview and rotate only when the developed
    // RAW has the opposite portrait/landscape layout.
    let (orientation, preview_portrait) = nef_preview_cached(path)
        .ok()
        .and_then(|(jpeg, orientation)| {
            image::load_from_memory(&jpeg).ok().map(|preview| {
                let preview = apply_orientation(preview, orientation);
                (orientation, preview.height() > preview.width())
            })
        })
        .unwrap_or((rawler_orientation, matches!(rawler_orientation, 5..=8)));
    let raw_portrait = img.height() > img.width();
    Ok(if raw_portrait == preview_portrait {
        img
    } else {
        apply_orientation(img, orientation)
    })
}

/// Load the full-resolution, correctly-oriented source used by final export.
/// Unlike `load_oriented_image`, NEF files are demosaiced from sensor data and
/// never fall back to the small embedded JPEG unless the caller chooses to.
pub(crate) fn load_export_image(path: &str) -> Result<image::DynamicImage, String> {
    let ext = std::path::Path::new(path)
        .extension().and_then(|value| value.to_str()).unwrap_or("")
        .to_ascii_lowercase();
    if ext == "nef" || ext == "nrw" { develop_nef_full(path) } else { load_oriented_image(path) }
}

fn render_nef_full_raw(path: &str, max: u32, quality: u8) -> Result<Vec<u8>, String> {
    let oriented = develop_nef_full(path)?;
    let resized = if oriented.width() <= max && oriented.height() <= max {
        oriented
    } else {
        // Lanczos3 retains noticeably more fine detail than CatmullRom when a
        // high-megapixel sensor image is reduced for the develop canvas.
        oriented.resize(max, max, image::imageops::FilterType::Lanczos3)
    };
    // RAW demosaicing intentionally produces a conservative, slightly soft
    // image. Apply a restrained capture-sharpening pass after resizing. This
    // improves foliage, hair and text detail without the halos caused by a
    // strong display sharpen. Raster files are not touched because they are
    // commonly already sharpened in-camera or during export.
    let sharpened = resized.unsharpen(0.75, 2);
    encode_jpeg(&sharpened, quality)
}

/// Generate a high-quality full-resolution preview for the develop viewer.
///
/// For NEF files this performs a *full* RAW develop (PPG demosaic + white
/// balance + camera color matrix + sRGB gamma) via `rawler`, so the centered
/// image is rendered at the sensor's native resolution rather than the camera's
/// small embedded JPEG preview. The result is cached on disk so the heavy decode
/// happens once per file version. If the RAW develop fails for any reason we
/// transparently fall back to the embedded-preview path so a preview is always
/// produced. Raster files (JPEG/TIFF) use the high-quality (q97) resize path.
/// Runs on the blocking pool, like `make_thumbnail`.
#[tauri::command]
pub async fn make_full_preview(path: String, max_size: Option<u32>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || -> Result<String, String> {
        // The frontend derives this from the fitted canvas size, display DPI
        // and zoom level. Bound it to predictable cache/memory limits; callers
        // from older builds still get a high-quality 4096px preview.
        let max = max_size.unwrap_or(4096).clamp(1024, 8192);
        let ext = std::path::Path::new(&path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext == "nef" {
            // Prefer a true full-resolution RAW develop for maximum sharpness.
            // The rendered JPEG is cached on disk.  Develop previews are sent
            // inline instead of through Tauri's asset protocol because the
            // frontend draws them onto a canvas and calls getImageData().  An
            // asset:// image is cross-origin in WebView2 and can taint that
            // canvas even though it displays correctly in a plain <img>.
            // Versioned cache kind invalidates previews made before the RAW
            // orientation and capture-sharpening fixes.
            if let Some(entry) = thumb_cache::entry_path(&path, max, "rawfull-v4") {
                if entry.exists() {
                    if let Some(bytes) = thumb_cache::read(&entry) {
                        return Ok(data_url(&bytes));
                    }
                }
                let buf = match render_nef_full_raw(&path, max, 96) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!("RAW develop failed for {path}, using embedded preview: {e}");
                        render_resized(&path, max, image::imageops::FilterType::Lanczos3, 95)?
                    }
                };
                // Cache best-effort; the current preview must not depend on the
                // asset protocol or on the cache write succeeding.
                let _ = thumb_cache::write(&entry, &buf);
                return Ok(data_url(&buf));
            }
            // No cache dir available: embedded preview, base64 inline.
            let buf = render_resized(&path, max, image::imageops::FilterType::Lanczos3, 95)?;
            return Ok(data_url(&buf));
        }

        // Version 2 preserves native JPEG bytes whenever no resize/orientation
        // transform is necessary, replacing the old quality-95 re-encode.
        if let Some(entry) = thumb_cache::entry_path(&path, max, "full-v2") {
            if entry.exists() {
                if let Some(bytes) = thumb_cache::read(&entry) {
                    return Ok(data_url(&bytes));
                }
            }
            let buf = render_raster_full_preview(&path, max)?;
            let _ = thumb_cache::write(&entry, &buf);
            return Ok(data_url(&buf));
        }
        let buf = render_raster_full_preview(&path, max)?;
        Ok(data_url(&buf))
    })
    .await
    .map_err(|e| format!("preview task failed: {e}"))?
}
/// Fast low-resolution preview used for progressive loading: the instant
/// placeholder shown in the develop viewer while the heavy full-resolution RAW
/// develop runs in the background.
///
/// For NEF this is the camera's embedded JPEG preview (oriented): it is tiny
/// (~0.3 MP) and its bytes are already cached by thumbnail generation, so it
/// decodes in a couple of milliseconds instead of the multi-second full
/// demosaic. For raster files (JPEG/PNG/TIFF) it is a downscaled render, which
/// is likewise fast. Cached on disk (kind "fastprev") so repeat views are
/// near-instant. Returns an inline data URL: unlike grid thumbnails, this image
/// is consumed by a canvas and therefore must be same-origin/readable.
fn render_fast_preview(path: &str) -> Result<String, String> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let render = || -> Result<Vec<u8>, String> {
        if ext == "nef" {
            // Oriented embedded preview JPEG (native ~0.3 MP), re-encoded.
            let (jpeg, orientation) = nef_preview_cached(path)?;
            let decoded = image::load_from_memory(&jpeg)
                .map_err(|e| format!("decode preview failed: {e}"))?;
            let oriented = apply_orientation(decoded, orientation);
            encode_jpeg(&oriented, 92)
        } else {
            render_resized(path, 1600, image::imageops::FilterType::Lanczos3, 92)
        }
    };

    if let Some(entry) = thumb_cache::entry_path(path, 0, "fastprev") {
        if entry.exists() {
            if let Some(bytes) = thumb_cache::read(&entry) {
                return Ok(data_url(&bytes));
            }
        }
        let buf = render()?;
        let _ = thumb_cache::write(&entry, &buf);
        return Ok(data_url(&buf));
    }
    // No cache directory available: inline base64.
    Ok(data_url(&render()?))
}

/// Generate a fast low-resolution preview for progressive loading. Pairs with
/// `make_full_preview`: the viewer shows this instantly, then swaps in the
/// full-resolution develop when it finishes. Runs on the blocking pool.
#[tauri::command]
pub async fn make_preview_fast(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || render_fast_preview(&path))
        .await
        .map_err(|e| format!("fast preview task failed: {e}"))?
}
/// One thumbnail result in a batch: the source path plus the absolute path of
/// its cached JPEG (`url`). The frontend converts that path to an asset-protocol
/// URL (convertFileSrc) so the bytes stream from disk instead of riding the IPC
/// payload as base64. `url` is an empty string when that particular file failed
/// to render, so a single bad file never aborts the whole batch.
#[derive(Debug, Clone, Serialize)]
pub struct ThumbEntry {
    pub path: String,
    pub url: String,
}

/// Render many thumbnails in parallel across CPU cores using a work-stealing
/// thread pool (the same pattern as batch blur detection). Each render is
/// backed by the on-disk cache, so repeat batches are near-instant. Results
/// preserve input order.
pub fn render_thumbnails_parallel(paths: &[String], max_size: u32) -> Vec<ThumbEntry> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let n = paths.len();
    let slots: Vec<Mutex<Option<String>>> = (0..n).map(|_| Mutex::new(None)).collect();
    let max_threads = std::thread::available_parallelism()
        .map(|x| x.get())
        .unwrap_or(4)
        .clamp(1, 12);
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..max_threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= n {
                    break;
                }
                let url = render_thumbnail_to_cache(&paths[i], max_size)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                *slots[i].lock().unwrap() = Some(url);
            });
        }
    });
    paths
        .iter()
        .enumerate()
        .map(|(i, p)| ThumbEntry {
            path: p.clone(),
            url: slots[i].lock().unwrap().take().unwrap_or_default(),
        })
        .collect()
}

/// Batch thumbnail generation: a single IPC round-trip renders thumbnails for
/// many photos in parallel across CPU cores and returns their cached file paths
/// (asset-protocol mode). The frontend serves those files via convertFileSrc, so
/// the IPC payload stays tiny (just paths) no matter how large the folder is.
/// Backed by the same on-disk cache; `make_thumbnail` remains the base64
/// fallback the frontend uses if an asset URL ever fails to load.
#[tauri::command]
pub async fn make_thumbnails_batch(
    paths: Vec<String>,
    max_size: Option<u32>,
) -> Result<Vec<ThumbEntry>, String> {
    let max = max_size.unwrap_or(320);
    tokio::task::spawn_blocking(move || render_thumbnails_parallel(&paths, max))
        .await
        .map_err(|e| format!("batch thumbnail task failed: {e}"))
}

/// Tiny dependency-free base64 encoder module.
mod base64_encode {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    pub fn encode(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = *chunk.get(1).unwrap_or(&0) as u32;
            let b2 = *chunk.get(2).unwrap_or(&0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(TABLE[(n >> 18) as usize & 63] as char);
            out.push(TABLE[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
            out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
        }
        out
    }
}

/// Extract embedded JPEG previews from Nikon NEF (TIFF-based) files.
mod nef {
    /// Minimal TIFF reader. Offsets stored in the file are relative to `base`,
    /// which lets us parse both the outer TIFF (base 0) and the embedded Nikon
    /// MakerNote TIFF (base = start of its "II/MM" header).
    struct Tiff<'a> {
        d: &'a [u8],
        base: usize,
        le: bool,
    }

    type Entry = (u16, u16, u32, u32); // (tag, type, count, value/offset field)

    impl<'a> Tiff<'a> {
        fn at(d: &'a [u8], base: usize) -> Option<Tiff<'a>> {
            let b0 = *d.get(base)?;
            let b1 = *d.get(base + 1)?;
            let le = match (b0, b1) {
                (0x49, 0x49) => true,
                (0x4d, 0x4d) => false,
                _ => return None,
            };
            let t = Tiff { d, base, le };
            if t.u16(base + 2)? != 42 {
                return None;
            }
            Some(t)
        }

        fn u16(&self, o: usize) -> Option<u16> {
            let b = self.d.get(o..o + 2)?;
            Some(if self.le {
                u16::from_le_bytes([b[0], b[1]])
            } else {
                u16::from_be_bytes([b[0], b[1]])
            })
        }

        fn u32(&self, o: usize) -> Option<u32> {
            let b = self.d.get(o..o + 4)?;
            Some(if self.le {
                u32::from_le_bytes([b[0], b[1], b[2], b[3]])
            } else {
                u32::from_be_bytes([b[0], b[1], b[2], b[3]])
            })
        }

        fn ifd0(&self) -> Option<usize> {
            let rel = self.u32(self.base + 4)?;
            Some(self.base + rel as usize)
        }

        /// Read an IFD at absolute offset `ifd_abs`. Returns its entries and the
        /// absolute offset of the next IFD (if any).
        fn read_ifd(&self, ifd_abs: usize) -> Option<(Vec<Entry>, Option<usize>)> {
            let n = self.u16(ifd_abs)? as usize;
            if n > 1000 {
                return None; // sanity guard
            }
            let mut entries = Vec::with_capacity(n);
            let mut p = ifd_abs + 2;
            for _ in 0..n {
                let tag = self.u16(p)?;
                let typ = self.u16(p + 2)?;
                let cnt = self.u32(p + 4)?;
                let val = self.u32(p + 8)?;
                entries.push((tag, typ, cnt, val));
                p += 12;
            }
            let next = self.u32(p).unwrap_or(0);
            let next_abs = if next == 0 { None } else { Some(self.base + next as usize) };
            Some((entries, next_abs))
        }

        /// Value of a LONG/SHORT count-1 entry as u32 (raw value field).
        fn get_val(&self, entries: &[Entry], tag: u16) -> Option<u32> {
            entries.iter().find(|(t, _, _, _)| *t == tag).map(|(_, _, _, v)| *v)
        }
    }

    fn valid_jpeg(d: &[u8], abs: usize, len: usize) -> bool {
        len >= 4
            && abs.checked_add(len).map(|e| e <= d.len()).unwrap_or(false)
            && d.get(abs) == Some(&0xff)
            && d.get(abs + 1) == Some(&0xd8)
    }

    /// Look for JPEGInterchangeFormat(0x0201)/Length(0x0202) in an IFD.
    fn jpeg_in_ifd(t: &Tiff, ifd_abs: usize) -> Option<(usize, usize)> {
        let (entries, _) = t.read_ifd(ifd_abs)?;
        let off = t.get_val(&entries, 0x0201)? as usize;
        let len = t.get_val(&entries, 0x0202)? as usize;
        let abs = t.base + off;
        if valid_jpeg(t.d, abs, len) {
            Some((abs, len))
        } else {
            None
        }
    }

    /// Parse a Nikon Type-3 MakerNote and return its large preview JPEG.
    fn nikon_makernote_preview(d: &[u8], mn_off: usize) -> Option<(usize, usize)> {
        if d.len() < mn_off + 10 {
            return None;
        }
        if &d[mn_off..mn_off + 6] != b"Nikon\0" {
            return None;
        }
        let base = mn_off + 10; // embedded TIFF header starts here
        let t = Tiff::at(d, base)?;
        let ifd0 = t.ifd0()?;
        let (entries, _) = t.read_ifd(ifd0)?;
        // Tag 0x0011 = PreviewIFD pointer (relative to embedded TIFF base).
        let prev_rel = t.get_val(&entries, 0x0011)?;
        let prev_abs = t.base + prev_rel as usize;
        jpeg_in_ifd(&t, prev_abs)
    }

    /// Extract the largest embedded JPEG preview from a NEF byte buffer.
    pub fn extract_preview(d: &[u8]) -> Option<Vec<u8>> {
        let t = Tiff::at(d, 0)?;
        let ifd0 = t.ifd0()?;
        let (entries0, next_ifd) = t.read_ifd(ifd0)?;
        let mut candidates: Vec<(usize, usize)> = Vec::new();

        // 1. EXIF thumbnail in IFD1 (next IFD after IFD0).
        if let Some(ifd1) = next_ifd {
            if let Some(c) = jpeg_in_ifd(&t, ifd1) {
                candidates.push(c);
            }
        }

        // 2. SubIFD referenced from IFD0 (tag 0x014A) may hold a preview JPEG.
        if let Some(sub_rel) = t.get_val(&entries0, 0x014A) {
            if let Some(c) = jpeg_in_ifd(&t, t.base + sub_rel as usize) {
                candidates.push(c);
            }
        }

        // 3. ExifIFD (0x8769) -> MakerNote (0x927C) -> Nikon PreviewIFD.
        if let Some(exif_rel) = t.get_val(&entries0, 0x8769) {
            let exif_abs = t.base + exif_rel as usize;
            if let Some((exif_entries, _)) = t.read_ifd(exif_abs) {
                if let Some(mn) = exif_entries.iter().find(|(tag, _, _, _)| *tag == 0x927C) {
                    let mn_off = t.base + mn.3 as usize;
                    if let Some(c) = nikon_makernote_preview(d, mn_off) {
                        candidates.push(c);
                    }
                }
            }
        }

        let (abs, len) = candidates.into_iter().max_by_key(|&(_, len)| len)?;
        Some(d[abs..abs + len].to_vec())
    }

    /// True if the slice starts with the JPEG SOI magic bytes (FF D8).
    pub fn is_jpeg(d: &[u8]) -> bool {
        d.len() >= 4 && d[0] == 0xff && d[1] == 0xd8
    }

    /// Locate the largest embedded preview JPEG by parsing ONLY the TIFF/IFD
    /// metadata (which lives near the start of a NEF). Returns the absolute
    /// file offset and length of the preview JPEG: (abs_file_offset, length).
    ///
    /// Unlike extract_preview, this bounds every candidate against the real
    /// file_size rather than the buffer length, and does NOT require the JPEG
    /// magic to be present inside the buffer. That lets it run on a small
    /// "head" read (the first ~1 MiB) where the actual JPEG payload lies far
    /// beyond the buffer, so we can learn the preview's offset without reading
    /// the whole 20-30 MB RAW file.
    pub fn find_preview_location(d: &[u8], file_size: usize) -> Option<(usize, usize)> {
        // JPEGInterchangeFormat(0x0201)/Length(0x0202) in an IFD, bounded by the
        // full file size (the JPEG payload may sit outside the head buffer).
        fn ifd_jpeg_loc(t: &Tiff, ifd_abs: usize, file_size: usize) -> Option<(usize, usize)> {
            let (entries, _) = t.read_ifd(ifd_abs)?;
            let off = t.get_val(&entries, 0x0201)? as usize;
            let len = t.get_val(&entries, 0x0202)? as usize;
            let abs = t.base + off;
            if len >= 4 && abs.checked_add(len).map(|e| e <= file_size).unwrap_or(false) {
                Some((abs, len))
            } else {
                None
            }
        }

        // Parse a Nikon Type-3 MakerNote and return its preview JPEG location.
        fn makernote_loc(d: &[u8], mn_off: usize, file_size: usize) -> Option<(usize, usize)> {
            if d.len() < mn_off + 10 {
                return None;
            }
            // "Nikon\0" header, written without a NUL byte literal.
            if &d[mn_off..mn_off + 5] != b"Nikon" || d[mn_off + 5] != 0 {
                return None;
            }
            let base = mn_off + 10; // embedded TIFF header starts here
            let t = Tiff::at(d, base)?;
            let ifd0 = t.ifd0()?;
            let (entries, _) = t.read_ifd(ifd0)?;
            // Tag 0x0011 = PreviewIFD pointer (relative to embedded TIFF base).
            let prev_rel = t.get_val(&entries, 0x0011)?;
            ifd_jpeg_loc(&t, t.base + prev_rel as usize, file_size)
        }

        let t = Tiff::at(d, 0)?;
        let ifd0 = t.ifd0()?;
        let (entries0, next_ifd) = t.read_ifd(ifd0)?;
        let mut candidates: Vec<(usize, usize)> = Vec::new();

        // 1. EXIF thumbnail in IFD1 (next IFD after IFD0).
        if let Some(ifd1) = next_ifd {
            if let Some(c) = ifd_jpeg_loc(&t, ifd1, file_size) {
                candidates.push(c);
            }
        }

        // 2. SubIFD referenced from IFD0 (tag 0x014A) may hold a preview JPEG.
        if let Some(sub_rel) = t.get_val(&entries0, 0x014A) {
            if let Some(c) = ifd_jpeg_loc(&t, t.base + sub_rel as usize, file_size) {
                candidates.push(c);
            }
        }

        // 3. ExifIFD (0x8769) -> MakerNote (0x927C) -> Nikon PreviewIFD.
        if let Some(exif_rel) = t.get_val(&entries0, 0x8769) {
            let exif_abs = t.base + exif_rel as usize;
            if let Some((exif_entries, _)) = t.read_ifd(exif_abs) {
                if let Some(mn) = exif_entries.iter().find(|(tag, _, _, _)| *tag == 0x927C) {
                    if let Some(c) = makernote_loc(d, t.base + mn.3 as usize, file_size) {
                        candidates.push(c);
                    }
                }
            }
        }

        candidates.into_iter().max_by_key(|&(_, len)| len)
    }
}

#[tauri::command]
pub async fn get_photo_info(path: String) -> Result<PhotoRecord, String> {
    let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    let filename = std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let format = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown")
        .to_uppercase();

    Ok(PhotoRecord {
        id: 0,
        file_path: path,
        filename,
        format,
        width: 0,
        height: 0,
        file_size: metadata.len(),
        date_taken: None,
        camera_model: None,
        lens: None,
        aperture: None,
        shutter_speed: None,
        iso: None,
        focal_length: None,
        blur_score: None,
        category: None,
        is_deleted: false,
        created_at: chrono_now(),
    })
}

#[tauri::command]
pub async fn get_exif(path: String) -> Result<ExifData, String> {
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let dims = if ext == "nef" {
        std::fs::read(&path)
            .ok()
            .and_then(|b| nef::extract_preview(&b))
            .and_then(|j| jpeg_dimensions(&j))
    } else {
        image::image_dimensions(&path).ok()
    };
    let (w, h) = dims.unwrap_or((5568, 3712));

    Ok(ExifData {
        camera_model: Some("Nikon Z30".to_string()),
        lens: Some("NIKKOR Z 16-50mm f/3.5-6.3 VR".to_string()),
        aperture: Some(3.5),
        shutter_speed: Some("1/250".to_string()),
        iso: Some(400),
        focal_length: Some(35.0),
        white_balance: Some("Auto".to_string()),
        date_taken: Some("2026:07:21 14:30:00".to_string()),
        width: w,
        height: h,
    })
}

fn chrono_now() -> String {
    "2026-07-21T00:00:00Z".to_string()
}
#[cfg(test)]
mod nef_tests {
    use super::nef::{extract_preview, find_preview_location, is_jpeg};
    use super::read_nef_preview_partial;

    fn le16(v: u16) -> [u8; 2] { v.to_le_bytes() }
    fn le32(v: u32) -> [u8; 4] { v.to_le_bytes() }

    /// Build a minimal little-endian TIFF with an EXIF thumbnail (IFD1) JPEG.
    #[test]
    fn extracts_ifd1_thumbnail() {
        let mut d = Vec::new();
        d.extend_from_slice(b"II");          // little-endian
        d.extend_from_slice(&le16(42));      // magic
        d.extend_from_slice(&le32(8));       // IFD0 @ 8
        // IFD0 @8: 1 entry
        d.extend_from_slice(&le16(1));
        d.extend_from_slice(&le16(0x0100));  // ImageWidth
        d.extend_from_slice(&le16(3));       // SHORT
        d.extend_from_slice(&le32(1));
        d.extend_from_slice(&le32(100));
        d.extend_from_slice(&le32(26));      // next IFD (IFD1) @26
        // IFD1 @26: 2 entries
        d.extend_from_slice(&le16(2));
        d.extend_from_slice(&le16(0x0201));  // JPEGInterchangeFormat
        d.extend_from_slice(&le16(4));       // LONG
        d.extend_from_slice(&le32(1));
        d.extend_from_slice(&le32(56));      // JPEG @56
        d.extend_from_slice(&le16(0x0202));  // JPEGInterchangeFormatLength
        d.extend_from_slice(&le16(4));
        d.extend_from_slice(&le32(1));
        d.extend_from_slice(&le32(8));       // length 8
        d.extend_from_slice(&le32(0));       // no next IFD
        while d.len() < 56 { d.push(0); }
        d.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE0, 0x11, 0x22, 0xFF, 0xD9]);

        let jpeg = extract_preview(&d).expect("should extract a preview");
        assert_eq!(jpeg.len(), 8);
        assert_eq!(&jpeg[0..2], &[0xFF, 0xD8]);
        assert_eq!(&jpeg[6..8], &[0xFF, 0xD9]);
    }

    #[test]
    fn returns_none_for_garbage() {
        assert!(extract_preview(&[0u8; 64]).is_none());
        assert!(extract_preview(b"not a tiff at all").is_none());
    }

    #[test]
    fn is_jpeg_detects_soi() {
        assert!(is_jpeg(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(!is_jpeg(&[0xFF, 0xD8, 0xFF])); // too short (< 4)
        assert!(!is_jpeg(&[0xFF, 0xD9, 0xFF, 0xE0])); // wrong magic
        assert!(!is_jpeg(&[]));
    }

    /// Build a little-endian TIFF whose IFD1 thumbnail JPEG sits at byte 56
    /// (8 bytes long) inside a 64-byte buffer.
    fn tiff_with_inbuffer_jpeg() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(b"II");
        d.extend_from_slice(&le16(42));
        d.extend_from_slice(&le32(8));       // IFD0 @ 8
        d.extend_from_slice(&le16(1));       // 1 entry
        d.extend_from_slice(&le16(0x0100));  // ImageWidth
        d.extend_from_slice(&le16(3));
        d.extend_from_slice(&le32(1));
        d.extend_from_slice(&le32(100));
        d.extend_from_slice(&le32(26));      // next IFD (IFD1) @26
        d.extend_from_slice(&le16(2));       // IFD1: 2 entries
        d.extend_from_slice(&le16(0x0201));
        d.extend_from_slice(&le16(4));
        d.extend_from_slice(&le32(1));
        d.extend_from_slice(&le32(56));      // JPEG @56
        d.extend_from_slice(&le16(0x0202));
        d.extend_from_slice(&le16(4));
        d.extend_from_slice(&le32(1));
        d.extend_from_slice(&le32(8));       // length 8
        d.extend_from_slice(&le32(0));       // no next IFD
        while d.len() < 56 { d.push(0); }
        d.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE0, 0x11, 0x22, 0xFF, 0xD9]);
        d
    }

    #[test]
    fn find_preview_location_in_buffer() {
        let d = tiff_with_inbuffer_jpeg();
        let loc = find_preview_location(&d, d.len()).expect("should locate preview");
        assert_eq!(loc, (56, 8));
        assert!(is_jpeg(&d[loc.0..loc.0 + loc.1]));
    }

    #[test]
    fn find_preview_location_works_beyond_buffer() {
        // IFD1 points to a JPEG at offset 5000 (len 900) which is FAR beyond
        // the 56-byte metadata buffer. extract_preview must fail (it bounds by
        // buffer length) but find_preview_location must succeed because it
        // bounds by the real file_size (6000) -- the core partial-read property.
        let mut d = Vec::new();
        d.extend_from_slice(b"II");
        d.extend_from_slice(&le16(42));
        d.extend_from_slice(&le32(8));       // IFD0 @ 8
        d.extend_from_slice(&le16(1));
        d.extend_from_slice(&le16(0x0100));
        d.extend_from_slice(&le16(3));
        d.extend_from_slice(&le32(1));
        d.extend_from_slice(&le32(100));
        d.extend_from_slice(&le32(26));      // IFD1 @26
        d.extend_from_slice(&le16(2));
        d.extend_from_slice(&le16(0x0201));
        d.extend_from_slice(&le16(4));
        d.extend_from_slice(&le32(1));
        d.extend_from_slice(&le32(5000));    // JPEG @5000 (outside buffer)
        d.extend_from_slice(&le16(0x0202));
        d.extend_from_slice(&le16(4));
        d.extend_from_slice(&le32(1));
        d.extend_from_slice(&le32(900));     // length 900
        d.extend_from_slice(&le32(0));
        assert_eq!(d.len(), 56);

        assert!(extract_preview(&d).is_none(), "full extractor cannot see past buffer");
        let loc = find_preview_location(&d, 6000).expect("partial locator must succeed");
        assert_eq!(loc, (5000, 900));
        // But if the claimed JPEG runs past the real file size, reject it.
        assert!(find_preview_location(&d, 5500).is_none());
    }

    #[test]
    fn read_nef_preview_partial_reads_file() {
        // A tiny "NEF" (TIFF with orientation=6 in IFD0 and an 8-byte JPEG in
        // IFD1) written to disk. The partial reader must return the JPEG bytes
        // and the orientation without a full-file extractor.
        let mut d = Vec::new();
        d.extend_from_slice(b"II");
        d.extend_from_slice(&le16(42));
        d.extend_from_slice(&le32(8));       // IFD0 @ 8
        d.extend_from_slice(&le16(1));
        d.extend_from_slice(&le16(0x0112));  // Orientation
        d.extend_from_slice(&le16(3));
        d.extend_from_slice(&le32(1));
        d.extend_from_slice(&le32(6));       // orientation = 6
        d.extend_from_slice(&le32(26));      // IFD1 @26
        d.extend_from_slice(&le16(2));
        d.extend_from_slice(&le16(0x0201));
        d.extend_from_slice(&le16(4));
        d.extend_from_slice(&le32(1));
        d.extend_from_slice(&le32(56));      // JPEG @56
        d.extend_from_slice(&le16(0x0202));
        d.extend_from_slice(&le16(4));
        d.extend_from_slice(&le32(1));
        d.extend_from_slice(&le32(8));       // length 8
        d.extend_from_slice(&le32(0));
        while d.len() < 56 { d.push(0); }
        d.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE0, 0x11, 0x22, 0xFF, 0xD9]);

        let dir = tempfile::tempdir().unwrap();
        let nef = dir.path().join("partial.nef");
        std::fs::write(&nef, &d).unwrap();

        let (jpeg, orientation) =
            read_nef_preview_partial(nef.to_str().unwrap()).expect("partial read should succeed");
        assert_eq!(orientation, 6);
        assert_eq!(jpeg.len(), 8);
        assert_eq!(&jpeg[0..2], &[0xFF, 0xD8]);
        assert_eq!(&jpeg[6..8], &[0xFF, 0xD9]);
    }

    /// Env-gated real-NEF check: the partial reader must return the same preview
    /// JPEG as the full-buffer extractor, while reading far less of the file.
    /// Set NPS_TEST_NEF to a real .NEF path to enable; otherwise it skips.
    #[test]
    fn partial_read_real_nef_if_available() {
        let path = match std::env::var("NPS_TEST_NEF") {
            Ok(p) if !p.is_empty() => p,
            _ => return,
        };
        if !std::path::Path::new(&path).exists() {
            return;
        }

        let (jpeg, orientation) = read_nef_preview_partial(&path).expect("partial read real NEF");
        assert!(is_jpeg(&jpeg), "partial reader returned non-JPEG bytes");
        assert!(jpeg.len() > 10_000, "preview JPEG suspiciously small: {}", jpeg.len());
        assert!((1..=8).contains(&orientation), "bad orientation {orientation}");

        // Must agree with the robust full-buffer extractor.
        let raw = std::fs::read(&path).unwrap();
        let full = extract_preview(&raw).expect("full extractor on real NEF");
        assert_eq!(jpeg.len(), full.len(), "partial and full reads must match");
        assert_eq!(jpeg, full, "partial and full preview bytes must be identical");

        // Timing sanity (warmed by the reads above): partial should be quick.
        let t0 = std::time::Instant::now();
        for _ in 0..3 {
            let _ = read_nef_preview_partial(&path);
        }
        eprintln!(
            "partial read avg {:?} for {} byte preview (file {} bytes)",
            t0.elapsed() / 3,
            jpeg.len(),
            raw.len()
        );
    }
}

#[cfg(test)]
mod orientation_tests {
    use super::*;
    use image::GenericImageView;

    #[test]
    fn test_read_tiff_orientation_le() {
        // Little-endian TIFF with orientation = 6 (rotate 90 CW)
        let mut d = Vec::new();
        d.extend_from_slice(b"II");
        d.extend_from_slice(&42u16.to_le_bytes());
        d.extend_from_slice(&8u32.to_le_bytes()); // IFD0 @ 8
        d.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
        d.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation tag
        d.extend_from_slice(&3u16.to_le_bytes()); // SHORT type
        d.extend_from_slice(&1u32.to_le_bytes()); // count
        d.extend_from_slice(&6u16.to_le_bytes()); // value = 6
        d.extend_from_slice(&[0, 0]); // padding
        d.extend_from_slice(&0u32.to_le_bytes()); // next IFD
        assert_eq!(read_tiff_orientation(&d), 6);
    }

    #[test]
    fn test_read_tiff_orientation_be() {
        // Big-endian TIFF with orientation = 3 (rotate 180)
        let mut d = Vec::new();
        d.extend_from_slice(b"MM");
        d.extend_from_slice(&42u16.to_be_bytes());
        d.extend_from_slice(&8u32.to_be_bytes());
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&0x0112u16.to_be_bytes());
        d.extend_from_slice(&3u16.to_be_bytes());
        d.extend_from_slice(&1u32.to_be_bytes());
        d.extend_from_slice(&3u16.to_be_bytes());
        d.extend_from_slice(&[0, 0]);
        d.extend_from_slice(&0u32.to_be_bytes());
        assert_eq!(read_tiff_orientation(&d), 3);
    }

    #[test]
    fn test_read_tiff_orientation_default() {
        assert_eq!(read_tiff_orientation(&[0u8; 4]), 1);
        assert_eq!(read_tiff_orientation(b"XX"), 1);
    }

    #[test]
    fn test_jpeg_exif_orientation_no_exif() {
        // Minimal JPEG with no APP1
        let jpeg = [0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02];
        assert_eq!(jpeg_exif_orientation(&jpeg), 1);
    }

    #[test]
    fn test_jpeg_exif_orientation_not_jpeg() {
        assert_eq!(jpeg_exif_orientation(b"not a jpeg"), 1);
        assert_eq!(jpeg_exif_orientation(&[]), 1);
    }

    /// Helper: create a 2x1 image with left=red, right=blue for pixel-level checks.
    fn marker_image() -> image::DynamicImage {
        let mut img = image::RgbImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgb([255, 0, 0])); // left = red
        img.put_pixel(1, 0, image::Rgb([0, 0, 255])); // right = blue
        image::DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn test_apply_orientation_identity() {
        let img = marker_image();
        let result = apply_orientation(img, 1);
        assert_eq!(result.width(), 2);
        assert_eq!(result.height(), 1);
        // Pixels unchanged
        assert_eq!(result.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(1, 0).0, [0, 0, 255, 255]);
    }

    #[test]
    fn test_apply_orientation_rotate90cw() {
        // Orientation 6 = rotate 90 CW: 2x1 -> 1x2
        // Original: [R, B] (left to right)
        // After 90 CW: top=R (left goes up), bottom=B (right goes down)
        let img = marker_image();
        let result = apply_orientation(img, 6);
        assert_eq!(result.width(), 1);
        assert_eq!(result.height(), 2);
        assert_eq!(result.get_pixel(0, 0).0, [255, 0, 0, 255]); // top = red (was left)
        assert_eq!(result.get_pixel(0, 1).0, [0, 0, 255, 255]); // bottom = blue (was right)
    }

    #[test]
    fn test_apply_orientation_rotate180() {
        // Orientation 3 = rotate 180: [R, B] -> [B, R]
        let img = marker_image();
        let result = apply_orientation(img, 3);
        assert_eq!(result.width(), 2);
        assert_eq!(result.height(), 1);
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 255, 255]); // left = blue (was right)
        assert_eq!(result.get_pixel(1, 0).0, [255, 0, 0, 255]); // right = red (was left)
    }

    #[test]
    fn test_apply_orientation_rotate270cw() {
        // Orientation 8 = rotate 270 CW (= 90 CCW): 2x1 -> 1x2
        // Original: [R, B] (left to right)
        // After 270 CW: top=B (right goes up), bottom=R (left goes down)
        let img = marker_image();
        let result = apply_orientation(img, 8);
        assert_eq!(result.width(), 1);
        assert_eq!(result.height(), 2);
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 255, 255]); // top = blue (was right)
        assert_eq!(result.get_pixel(0, 1).0, [255, 0, 0, 255]); // bottom = red (was left)
    }

    #[test]
    fn test_apply_orientation_fliph() {
        // Orientation 2 = flip horizontal: [R, B] -> [B, R]
        let img = marker_image();
        let result = apply_orientation(img, 2);
        assert_eq!(result.width(), 2);
        assert_eq!(result.height(), 1);
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(result.get_pixel(1, 0).0, [255, 0, 0, 255]);
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize tests that mutate the process-wide USERPROFILE env var so they
    /// can't race each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Restore a process-wide env var to its original value on drop (even on
    /// panic), so a test that redirects USERPROFILE never pollutes the cache dir
    /// seen by other concurrently-running tests.
    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn write_test_png(path: &std::path::Path, w: u32, h: u32) {
        let mut g = vec![0u8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                g[(y * w + x) as usize] = ((x * 8 + y) % 256) as u8;
            }
        }
        let img = image::ImageBuffer::<image::Luma<u8>, Vec<u8>>::from_raw(w, h, g).expect("buf");
        image::DynamicImage::ImageLuma8(img).save(path).expect("save png");
    }

    #[test]
    fn test_encode_jpeg_produces_jpeg_bytes() {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(16, 16, image::Rgb([200, 100, 50])));
        let bytes = encode_jpeg(&img, 90).expect("encode");
        assert!(bytes.len() > 4, "jpeg should be non-trivial");
        assert_eq!(bytes[0], 0xFF, "JPEG SOI byte 0");
        assert_eq!(bytes[1], 0xD8, "JPEG SOI byte 1");
    }

    #[test]
    fn test_render_resized_downscales_to_fit() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.png");
        write_test_png(&p, 200, 100);
        let bytes = render_resized(p.to_str().unwrap(), 50, image::imageops::FilterType::Triangle, 90)
            .expect("render");
        let decoded = image::load_from_memory(&bytes).expect("decode rendered jpeg");
        assert!(decoded.width() <= 50 && decoded.height() <= 50,
            "rendered {}x{} should fit within 50px", decoded.width(), decoded.height());
    }

    #[test]
    fn test_full_preview_preserves_native_jpeg_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("native.jpg");
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(96, 64, |x, y| {
            image::Rgb([(x * 2) as u8, (y * 3) as u8, ((x + y) * 2) as u8])
        }));
        image.save_with_format(&path, image::ImageFormat::Jpeg).expect("save jpeg");
        let original = std::fs::read(&path).expect("read original");
        let preview = render_raster_full_preview(path.to_str().unwrap(), 2048).expect("render full preview");
        assert_eq!(preview, original, "native JPEG must not be decoded and re-encoded");
    }

    #[test]
    fn test_data_url_has_prefix() {
        let url = data_url(&[1, 2, 3]);
        assert!(url.starts_with("data:image/jpeg;base64,"));
    }

    #[test]
    fn test_thumb_cache_roundtrip_and_invalidation() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        // Isolate the cache under a temp "home", restoring the original value
        // (even on panic) so we never pollute the cache dir for other tests.
        let _guard = EnvGuard {
            key: "USERPROFILE",
            original: std::env::var("USERPROFILE").ok(),
        };
        std::env::set_var("USERPROFILE", dir.path());

        let src = dir.path().join("photo.png");
        write_test_png(&src, 64, 64);
        let src_str = src.to_str().unwrap();

        // Deterministic key: identical params -> identical entry.
        let e1 = thumb_cache::entry_path(src_str, 320, "thumb").expect("entry");
        let e2 = thumb_cache::entry_path(src_str, 320, "thumb").expect("entry");
        assert_eq!(e1, e2, "same params must map to the same cache file");

        // Different render size -> different entry.
        let e3 = thumb_cache::entry_path(src_str, 160, "thumb").expect("entry");
        assert_ne!(e1, e3, "different size must map to a different cache file");

        // Miss before write, hit after.
        assert!(thumb_cache::read(&e1).is_none(), "cold cache should miss");
        thumb_cache::write(&e1, b"hello-cache");
        assert_eq!(thumb_cache::read(&e1).unwrap(), b"hello-cache");

        // Editing the source (different size) invalidates -> new key.
        write_test_png(&src, 128, 128);
        let e4 = thumb_cache::entry_path(src_str, 320, "thumb").expect("entry");
        assert_ne!(e1, e4, "changing the source file must change the cache key");
    }

    #[test]
    fn test_load_oriented_image_uses_cache_for_raster() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("img.png");
        write_test_png(&p, 40, 30);
        let img = load_oriented_image(p.to_str().unwrap()).expect("load");
        assert_eq!(img.width(), 40);
        assert_eq!(img.height(), 30);
    }

    #[test]
    fn test_render_thumbnails_parallel_preserves_order() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for i in 0..6u32 {
            let p = dir.path().join(format!("img_{i}.png"));
            write_test_png(&p, 40 + i, 30);
            paths.push(p.to_str().unwrap().to_string());
        }
        let out = render_thumbnails_parallel(&paths, 64);
        assert_eq!(out.len(), 6, "one entry per input path");
        for (i, e) in out.iter().enumerate() {
            assert_eq!(e.path, paths[i], "input order must be preserved");
            assert!(
                !e.url.is_empty() && std::path::Path::new(&e.url).exists(),
                "entry {i} should be a rendered cache file path that exists"
            );
            assert!(e.url.ends_with(".thumb"), "entry {i} should be a .thumb cache file");
        }
    }

    #[test]
    fn test_render_thumbnails_parallel_tolerates_failures() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good.png");
        write_test_png(&good, 32, 32);
        let missing = dir.path().join("does_not_exist.png");
        let paths = vec![
            good.to_str().unwrap().to_string(),
            missing.to_str().unwrap().to_string(),
        ];
        let out = render_thumbnails_parallel(&paths, 64);
        assert_eq!(out.len(), 2);
        assert!(
            !out[0].url.is_empty() && std::path::Path::new(&out[0].url).exists(),
            "good file renders to an existing cache path"
        );
        assert_eq!(out[1].url, "", "missing file yields empty url, not an abort");
        assert_eq!(out[1].path, paths[1], "failed entry still reports its path");
    }

    #[test]
    fn test_render_thumbnails_parallel_empty_input() {
        let out = render_thumbnails_parallel(&[], 64);
        assert!(out.is_empty(), "empty input yields empty output");
    }
    #[test]
    fn test_render_fast_preview_raster_roundtrip() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _guard = EnvGuard {
            key: "USERPROFILE",
            original: std::env::var("USERPROFILE").ok(),
        };
        std::env::set_var("USERPROFILE", dir.path());

        let src = dir.path().join("photo.png");
        write_test_png(&src, 200, 120);
        let src_str = src.to_str().unwrap();

        // Fast previews are deliberately returned inline so WebView2 can read
        // their pixels from a canvas without cross-origin tainting. The cache
        // remains an implementation detail behind the stable data URL API.
        let url1 = render_fast_preview(src_str).expect("fast preview renders");
        assert!(url1.starts_with("data:image/jpeg;base64,"));
        let cache = thumb_cache::entry_path(src_str, 0, "fastprev").expect("cache entry");
        assert!(cache.exists(), "cache file exists");
        let bytes = std::fs::read(cache).expect("read cache");
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "JPEG SOI bytes");

        // Second call is a cache hit -> identical path.
        let url2 = render_fast_preview(src_str).expect("fast preview cached");
        assert_eq!(url1, url2, "cache hit returns identical inline bytes");
    }

    /// Integration check (only when NPS_TEST_NEF points at a real NEF): the fast
    /// preview must be the camera's small embedded JPEG (oriented), NOT a full
    /// RAW demosaic, so it loads in milliseconds. Skips silently when unset.
    #[test]
    fn fast_preview_real_nef_is_embedded_preview() {
        let path = match std::env::var("NPS_TEST_NEF") {
            Ok(p) if !p.is_empty() => p,
            _ => {
                eprintln!("NPS_TEST_NEF not set; skipping real-NEF fast-preview test");
                return;
            }
        };
        if !std::path::Path::new(&path).exists() {
            eprintln!("NEF not found, skipping: {path}");
            return;
        }
        let t0 = std::time::Instant::now();
        let url = render_fast_preview(&path).expect("fast preview should succeed");
        let elapsed = t0.elapsed();
        let bytes = std::fs::read(&url).expect("read fast preview cache");
        let img = image::load_from_memory(&bytes).expect("valid JPEG");
        eprintln!(
            "fast preview OK: {}x{} in {:.1} ms ({} KB)",
            img.width(), img.height(), elapsed.as_secs_f64() * 1000.0, bytes.len() / 1024
        );
        // The embedded preview is small (~0.3 MP); a full demosaic would be
        // thousands of px across. Guard that we took the fast embedded path.
        let mp = img.width() as f64 * img.height() as f64;
        assert!(
            mp < 2_000_000.0,
            "fast preview should be the small embedded JPEG, got {}x{}",
            img.width(), img.height()
        );
    }
}

#[cfg(test)]
mod rawler_orientation_tests {
    use super::rawler_orientation_to_u8;
    use rawler::decoders::Orientation as O;

    /// The RAW develop path must orient portraits/rotated shots exactly like the
    /// embedded-preview path (which keys off the standard TIFF orientation code).
    /// Verify every rawler variant maps to the matching 1-8 code.
    #[test]
    fn maps_every_orientation_to_tiff_code() {
        assert_eq!(rawler_orientation_to_u8(O::Normal), 1);
        assert_eq!(rawler_orientation_to_u8(O::Unknown), 1);
        assert_eq!(rawler_orientation_to_u8(O::HorizontalFlip), 2);
        assert_eq!(rawler_orientation_to_u8(O::Rotate180), 3);
        assert_eq!(rawler_orientation_to_u8(O::VerticalFlip), 4);
        assert_eq!(rawler_orientation_to_u8(O::Transpose), 5);
        assert_eq!(rawler_orientation_to_u8(O::Rotate90), 6);
        assert_eq!(rawler_orientation_to_u8(O::Transverse), 7);
        assert_eq!(rawler_orientation_to_u8(O::Rotate270), 8);
    }

    /// Round-trip: rawler's own from_u16 inverse must agree with our mapping, so
    /// a NEF tagged with orientation N renders with the same transform N.
    #[test]
    fn round_trips_with_rawler_from_u16() {
        for code in 1u16..=8 {
            let mapped = rawler_orientation_to_u8(O::from_u16(code));
            assert_eq!(mapped, code as u8, "orientation {code} should round-trip");
        }
    }

    /// Integration check (runs only when NPS_TEST_NEF points at a real NEF):
    /// the full RAW develop must produce a high-resolution, valid JPEG — i.e.
    /// far larger than the camera's small embedded preview. Skips silently when
    /// the env var is unset so CI / other machines stay green.
    #[test]
    fn full_raw_decode_real_nef_if_available() {
        let path = match std::env::var("NPS_TEST_NEF") {
            Ok(p) if !p.is_empty() => p,
            _ => {
                eprintln!("NPS_TEST_NEF not set; skipping real-NEF decode test");
                return;
            }
        };
        if !std::path::Path::new(&path).exists() {
            eprintln!("NEF not found, skipping: {path}");
            return;
        }
        let start = std::time::Instant::now();
        let jpeg = super::render_nef_full_raw(&path, 4096, 92).expect("full RAW decode should succeed");
        let elapsed = start.elapsed();
        let img = image::load_from_memory(&jpeg).expect("produced bytes must be a valid JPEG");
        eprintln!(
            "RAW decode OK: {}x{} in {:.2}s, jpeg {} KB",
            img.width(),
            img.height(),
            elapsed.as_secs_f32(),
            jpeg.len() / 1024
        );
        assert!(jpeg.len() > 50_000, "full-res JPEG should be substantial");
        assert!(
            img.width() >= 2000 || img.height() >= 2000,
            "full RAW decode should be high resolution, got {}x{}",
            img.width(),
            img.height()
        );
    }
}

#[cfg(test)]
mod bench_tests {
    /// Env-gated benchmark (NOT a pass/fail test): set NPS_BENCH_DIR to a folder
    /// of NEFs to measure the batch-thumbnail pipeline -- the exact work done
    /// when a folder is opened in the grid. It isolates the three things a user
    /// perceives as "loading speed":
    ///   1. COLD first-open  (empty on-disk cache -> full NEF-preview decode)
    ///   2. WARM re-open     (cache hit -> bytes stream straight from disk)
    ///   3. parallel speedup (work-stealing pool vs a single-threaded loop)
    ///
    /// The COLD pass clears only the specific cache entries for the bench files
    /// (no global env mutation), so it is a genuine first-open and never races
    /// with the cache_tests that redirect USERPROFILE. Skips silently when
    /// NPS_BENCH_DIR is unset.
    #[test]
    fn bench_batch_thumbnails() {
        let dir = match std::env::var("NPS_BENCH_DIR") {
            Ok(d) if !d.is_empty() => d,
            _ => {
                eprintln!("NPS_BENCH_DIR not set; skipping benchmark");
                return;
            }
        };
        let limit: usize = std::env::var("NPS_BENCH_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(usize::MAX);

        let mut paths: Vec<String> = std::fs::read_dir(&dir)
            .expect("read bench dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x.eq_ignore_ascii_case("nef"))
                    .unwrap_or(false)
            })
            .filter_map(|p| p.to_str().map(|s| s.to_string()))
            .take(limit)
            .collect();
        paths.sort();
        let n = paths.len();
        if n == 0 {
            eprintln!("no NEF files in {dir}");
            return;
        }

        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        let size = 360u32;

        // Clear only these files' thumbnail cache entries so the COLD pass is a
        // genuine first-open no matter how many times the bench has run before.
        for p in &paths {
            if let Some(entry) = super::thumb_cache::entry_path(p, size, "thumb") {
                let _ = std::fs::remove_file(&entry);
            }
        }

        // COLD first-open: empty cache -> every thumbnail decodes the NEF preview.
        let c0 = std::time::Instant::now();
        let cold = super::render_thumbnails_parallel(&paths, size);
        let cold_dur = c0.elapsed();

        // WARM re-open: cache now populated -> bytes stream straight from disk.
        let w0 = std::time::Instant::now();
        let warm = super::render_thumbnails_parallel(&paths, size);
        let warm_dur = w0.elapsed();

        // Serial (warm cache) to quantify the multi-core parallel speedup.
        let s0 = std::time::Instant::now();
        let serial_ok = paths
            .iter()
            .filter(|p| super::render_thumbnail_to_cache(p, size).is_ok())
            .count();
        let serial_dur = s0.elapsed();

        let cold_ok = cold.iter().filter(|e| !e.url.is_empty()).count();
        let warm_ok = warm.iter().filter(|e| !e.url.is_empty()).count();
        let payload: usize = warm.iter().map(|e| e.url.len()).sum();
        eprintln!("=== BATCH THUMBNAIL BENCH ({n} NEFs, size {size}) ===");
        eprintln!("COLD first-open (empty cache): {cold_dur:?}  ({:.1} ms/thumb)", ms(cold_dur) / n as f64);
        eprintln!("WARM re-open    (cache hit)  : {warm_dur:?}  ({:.2} ms/thumb)", ms(warm_dur) / n as f64);
        eprintln!("serial WARM     (1 thread)   : {serial_dur:?}  ({:.2} ms/thumb)", ms(serial_dur) / n as f64);
        eprintln!("  -> parallel speedup vs serial: {:.2}x", ms(serial_dur) / ms(warm_dur).max(1e-9));
        eprintln!("rendered: cold={cold_ok} warm={warm_ok} serial={serial_ok}");
        eprintln!("IPC payload (asset paths): {:.1} KB total, {:.0} B/thumb avg",
            payload as f64 / 1024.0, payload as f64 / warm_ok.max(1) as f64);
    }
}
