//! Blur detection using multiple image analysis methods for high accuracy.
//!
//! Combined score (0-100, higher = sharper):
//!   - Multi-scale Laplacian variance  (weight 0.30)
//!   - Tenengrad gradient energy       (weight 0.25)
//!   - Sobel gradient magnitude        (weight 0.20)
//!   - FFT high-frequency ratio        (weight 0.10)
//!   - Local contrast (block std-dev)  (weight 0.15)

use super::{BlurResult, Sensitivity};

const W_LAPLACIAN: f32 = 0.30;
const W_TENENGRADE: f32 = 0.25;
const W_SOBEL: f32 = 0.20;
const W_FFT: f32 = 0.10;
const W_LOCAL_CONTRAST: f32 = 0.15;

#[derive(Debug, Clone)]
pub struct BlurScores {
    pub combined: f32,
    pub laplacian_var: f32,
    pub gradient_energy: f32,
    pub fft_hf_ratio: f32,
    pub tenengrad: f32,
    pub local_contrast: f32,
}

/// Compute blur score for a grayscale image buffer (pixel values 0-255).
pub fn compute_blur_score(gray: &[f32], width: usize, height: usize) -> BlurScores {
    if width < 3 || height < 3 {
        return BlurScores { combined: 0.0, laplacian_var: 0.0, gradient_energy: 0.0, fft_hf_ratio: 0.0, tenengrad: 0.0, local_contrast: 0.0 };
    }

    // --- Multi-scale Laplacian: original + 2x downsampled ---
    let lap1 = laplacian_variance(gray, width, height);
    let (gray2, w2, h2) = downsample2x(gray, width, height);
    let lap2 = if w2 >= 3 && h2 >= 3 { laplacian_variance(&gray2, w2, h2) } else { 0.0 };
    let lap_combined = lap1 * 0.6 + lap2 * 0.4;

    let (tenengrad, sobel) = gradient_energies(gray, width, height);
    let fft = fft_high_freq_ratio(gray, width, height);
    let lc = local_contrast(gray, width, height);

    // Normalize each metric to 0-100
    let lap_norm  = (lap_combined / 400.0 * 100.0).min(100.0);
    let ten_norm  = (tenengrad / 300.0 * 100.0).min(100.0);
    let sob_norm  = (sobel / 180.0 * 100.0).min(100.0);
    let fft_norm  = (fft * 100.0).min(100.0);
    let lc_norm   = (lc / 60.0 * 100.0).min(100.0);

    let combined = lap_norm * W_LAPLACIAN
        + ten_norm * W_TENENGRADE
        + sob_norm * W_SOBEL
        + fft_norm * W_FFT
        + lc_norm * W_LOCAL_CONTRAST;

    BlurScores {
        combined: combined.clamp(0.0, 100.0),
        laplacian_var: lap_combined,
        gradient_energy: sobel,
        fft_hf_ratio: fft,
        tenengrad,
        local_contrast: lc,
    }
}

// ---------- helpers ----------

fn downsample2x(gray: &[f32], w: usize, h: usize) -> (Vec<f32>, usize, usize) {
    let nw = w / 2;
    let nh = h / 2;
    if nw < 1 || nh < 1 { return (vec![], 0, 0); }
    let mut out = vec![0.0f32; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            let s = y * 2 * w + x * 2;
            out[y * nw + x] = (gray[s] + gray[s + 1] + gray[s + w] + gray[s + w + 1]) * 0.25;
        }
    }
    (out, nw, nh)
}

/// Laplacian variance (3x3 kernel [0,1,0; 1,-4,1; 0,1,0]).
pub fn laplacian_variance(gray: &[f32], width: usize, height: usize) -> f32 {
    if width < 3 || height < 3 { return 0.0; }
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut count = 0u64;
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let i = y * width + x;
            let lap = gray[i - width] + gray[i - 1] - 4.0 * gray[i] + gray[i + 1] + gray[i + width];
            sum += lap as f64;
            sum_sq += (lap as f64) * (lap as f64);
            count += 1;
        }
    }
    if count == 0 { return 0.0; }
    let mean = sum / count as f64;
    (sum_sq / count as f64 - mean * mean).max(0.0) as f32
}

/// Fused Sobel gradient pass computing both the Tenengrad and the Sobel
/// gradient-energy metrics in a single sweep over the interior pixels.
///
/// Tenengrad and Sobel use the exact same 3x3 Sobel gx/gy kernels; the old code
/// ran two separate identical passes. Computing gx/gy once and accumulating both
/// metrics from the same values removes that duplication (roughly halving the
/// gradient cost) while staying bit-for-bit identical: each metric accumulates in
/// the same row-major order with the same operations as before.
///
/// Returns (tenengrad, sobel):
///   - tenengrad = sqrt( mean of squared magnitude over pixels with mag^2 > 100 )
///   - sobel     = mean gradient magnitude over all interior pixels
fn gradient_energies(gray: &[f32], width: usize, height: usize) -> (f32, f32) {
    if width < 3 || height < 3 { return (0.0, 0.0); }
    let mut ten_total = 0.0f64;
    let mut ten_count = 0u64;
    let mut sob_total = 0.0f64;
    let mut sob_count = 0u64;
    let threshold_sq = 100.0f64; // ignore very weak gradients (noise floor)
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let i = y * width + x;
            let gx = -gray[i - width - 1] + gray[i - width + 1]
                - 2.0 * gray[i - 1] + 2.0 * gray[i + 1]
                - gray[i + width - 1] + gray[i + width + 1];
            let gy = -gray[i - width - 1] - 2.0 * gray[i - width] - gray[i - width + 1]
                + gray[i + width - 1] + 2.0 * gray[i + width] + gray[i + width + 1];
            let mag_sq = (gx * gx + gy * gy) as f64;
            // Sobel: mean gradient magnitude over every interior pixel.
            sob_total += mag_sq.sqrt();
            sob_count += 1;
            // Tenengrad: mean squared magnitude above the noise floor, then sqrt.
            if mag_sq > threshold_sq {
                ten_total += mag_sq;
                ten_count += 1;
            }
        }
    }
    let tenengrad = if ten_count == 0 { 0.0 } else { (ten_total / ten_count as f64).sqrt() as f32 };
    let sobel = if sob_count == 0 { 0.0 } else { (sob_total / sob_count as f64) as f32 };
    (tenengrad, sobel)
}

/// Tenengrad: sum of squared Sobel gradient magnitudes above a threshold.
/// More robust to noise than raw Sobel energy.
pub fn tenengrad_energy(gray: &[f32], width: usize, height: usize) -> f32 {
    gradient_energies(gray, width, height).0
}

/// Sobel gradient energy: average magnitude of spatial gradients.
pub fn sobel_gradient_energy(gray: &[f32], width: usize, height: usize) -> f32 {
    gradient_energies(gray, width, height).1
}
/// Simplified FFT high-frequency ratio.
pub fn fft_high_freq_ratio(gray: &[f32], width: usize, height: usize) -> f32 {
    if width < 4 || height < 4 { return 0.0; }
    let mean: f32 = gray.iter().sum::<f32>() / gray.len() as f32;
    let mut total_var = 0.0f64;
    let mut hf_energy = 0.0f64;
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let diff = gray[i] - mean;
            total_var += (diff * diff) as f64;
            if x > 0 && y > 0 {
                let dx = gray[i] - gray[i - 1];
                let dy = gray[i] - gray[i - width];
                hf_energy += (dx * dx + dy * dy) as f64;
            }
        }
    }
    if total_var < 1e-6 { return 0.0; }
    (hf_energy / (total_var * 4.0)).min(1.0) as f32
}

/// Local contrast: average standard deviation within non-overlapping 8x8 blocks.
/// Sharp images have high local contrast; blurry images have low.
pub fn local_contrast(gray: &[f32], width: usize, height: usize) -> f32 {
    let block = 8usize;
    if width < block || height < block { return 0.0; }
    let mut total_std = 0.0f64;
    let mut blocks = 0u64;
    let mut by = 0;
    while by + block <= height {
        let mut bx = 0;
        while bx + block <= width {
            let mut sum = 0.0f64;
            let mut sum_sq = 0.0f64;
            let n = (block * block) as f64;
            for dy in 0..block {
                for dx in 0..block {
                    let v = gray[(by + dy) * width + bx + dx] as f64;
                    sum += v;
                    sum_sq += v * v;
                }
            }
            let mean = sum / n;
            let var = (sum_sq / n - mean * mean).max(0.0);
            total_std += var.sqrt();
            blocks += 1;
            bx += block;
        }
        by += block;
    }
    if blocks == 0 { return 0.0; }
    (total_std / blocks as f64) as f32
}

/// Determine if a photo is blurry based on score and sensitivity.
pub fn is_blurry(score: f32, sensitivity: Sensitivity) -> bool {
    score < sensitivity.threshold()
}

/// Max analysis dimension for the standard-accuracy path. NEF embedded previews
/// are typically <= 640px on the long edge, so in practice the standard path
/// analyzes the preview at its native resolution (no upscale); the sensitivity
/// thresholds are calibrated against exactly this resolution.
const ANALYSIS_DIM: u32 = 1024;
/// Max analysis dimension for the accelerated (fast) path. The preview is
/// downscaled to this long edge before scoring, so every metric pass touches far
/// fewer pixels (~4x fewer than the standard path on a 640px preview). This is
/// what makes the accelerated toggle genuinely faster, not just a relabel.
const ANALYSIS_DIM_FAST: u32 = 320;

/// Load a downscaled grayscale buffer suitable for blur analysis.
///
/// Uses the shared oriented-image loader (NEF -> embedded JPEG preview) so RAW
/// files are decoded quickly instead of a full demosaic, and so the score is
/// computed on the same pixels the user actually sees.
fn load_analysis_gray(path: &str, max_dim: u32) -> Result<(Vec<f32>, usize, usize), String> {
    let img = crate::library::commands::load_oriented_image(path)?;
    let gray_img = img.to_luma8();
    let (w, h) = gray_img.dimensions();
    let (gray, w, h) = if w > max_dim || h > max_dim {
        let scale = max_dim as f32 / w.max(h) as f32;
        let nw = ((w as f32 * scale) as usize).max(1);
        let nh = ((h as f32 * scale) as usize).max(1);
        let resized = image::imageops::resize(&gray_img, nw as u32, nh as u32, image::imageops::FilterType::Triangle);
        let buf: Vec<f32> = resized.as_raw().iter().map(|&p| p as f32).collect();
        (buf, nw, nh)
    } else {
        let buf: Vec<f32> = gray_img.as_raw().iter().map(|&p| p as f32).collect();
        (buf, w as usize, h as usize)
    };
    Ok((gray, w, h))
}

/// Load the analysis grayscale buffer, transparently using the on-disk cache so
/// repeat detection runs skip the expensive NEF-preview JPEG decode entirely.
///
/// On a cache hit this reads a few hundred KB of raw pixels from disk (sub-ms)
/// instead of re-reading the large RAW file and re-decoding the preview JPEG
/// (~100ms in unoptimized builds). On a miss it decodes via load_analysis_gray
/// and populates the cache for next time. The cached pixels are the exact integer
/// 0-255 gray values, so the resulting score is identical to a fresh decode.
fn load_analysis_gray_cached(path: &str, max_dim: u32) -> Result<(Vec<f32>, usize, usize), String> {
    if let Some(cached) = crate::library::commands::analysis_cache_read(path, max_dim) {
        return Ok(cached);
    }
    let (mut gray, w, h) = load_analysis_gray(path, max_dim)?;
    // Quantize to integer 0-255 before scoring AND caching. The decode yields
    // integer pixels, but the downscale filter produces fractional values;
    // rounding here makes the u8 cache round-trip bit-identical to the buffer we
    // actually score, so a cold run and a cached re-run give the exact same score.
    for v in gray.iter_mut() {
        *v = v.round().clamp(0.0, 255.0);
    }
    crate::library::commands::analysis_cache_write(path, max_dim, &gray, w, h);
    Ok((gray, w, h))
}

/// Analyze a full image file for blur (standard accuracy).
pub fn analyze_file(path: &str, sensitivity: Sensitivity) -> Result<BlurResult, String> {
    analyze_file_with_options(path, sensitivity, false)
}

/// Analyze with an optional accelerated (fast) path: a smaller analysis buffer
/// trades a little precision for a large speed-up. Enabled when GPU/fast
/// acceleration is requested in the UI.
pub fn analyze_file_with_options(
    path: &str,
    sensitivity: Sensitivity,
    accelerated: bool,
) -> Result<BlurResult, String> {
    let max_dim = if accelerated { ANALYSIS_DIM_FAST } else { ANALYSIS_DIM };
    let (gray, w, h) = load_analysis_gray_cached(path, max_dim)?;
    let scores = compute_blur_score(&gray, w, h);
    Ok(BlurResult {
        file_path: path.to_string(),
        score: scores.combined,
        laplacian_var: scores.laplacian_var,
        gradient_energy: scores.gradient_energy,
        fft_hf_ratio: scores.fft_hf_ratio,
        is_blurry: is_blurry(scores.combined, sensitivity),
    })
}

/// Analyze many files in parallel across worker threads (work-stealing via an
/// atomic counter). Returns one `BlurResult` per successfully-analyzed file
/// (files that fail to decode are skipped). `accelerated` enables the fast path.
///
/// This replaces the old one-IPC-call-per-photo loop and is the main speed-up
/// for large libraries.
pub fn analyze_batch_parallel(
    paths: &[String],
    sensitivity: Sensitivity,
    accelerated: bool,
) -> Vec<BlurResult> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    let n = paths.len();
    if n == 0 {
        return Vec::new();
    }

    let max_threads = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4);
    // No point exceeding the work count; keep the pool sane.
    let threads = max_threads.min(n).min(12).max(1);

    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<BlurResult>> = Mutex::new(Vec::with_capacity(n));

    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let idx = next.fetch_add(1, Ordering::Relaxed);
                if idx >= n {
                    break;
                }
                if let Ok(res) = analyze_file_with_options(&paths[idx], sensitivity, accelerated) {
                    if let Ok(mut g) = out.lock() {
                        g.push(res);
                    }
                }
            });
        }
    });

    out.into_inner().unwrap_or_default()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_laplacian_variance_uniform() {
        let gray = vec![128.0f32; 100];
        let var = laplacian_variance(&gray, 10, 10);
        assert!(var < 0.001, "Uniform image should have ~0 Laplacian variance, got {}", var);
    }

    #[test]
    fn test_laplacian_variance_edges() {
        let mut gray = vec![0.0f32; 100];
        for y in 0..10 { for x in 5..10 { gray[y * 10 + x] = 255.0; } }
        let var = laplacian_variance(&gray, 10, 10);
        assert!(var > 100.0, "Sharp edge should have high variance, got {}", var);
    }

    #[test]
    fn test_sobel_gradient_uniform() {
        let gray = vec![100.0f32; 100];
        let energy = sobel_gradient_energy(&gray, 10, 10);
        assert!(energy < 0.001);
    }

    #[test]
    fn test_sobel_gradient_edge() {
        let mut gray = vec![0.0f32; 100];
        for y in 0..10 { for x in 5..10 { gray[y * 10 + x] = 255.0; } }
        let energy = sobel_gradient_energy(&gray, 10, 10);
        assert!(energy > 50.0, "Edge should have high gradient energy, got {}", energy);
    }

    #[test]
    fn test_fft_hf_ratio_uniform() {
        let gray = vec![128.0f32; 100];
        let ratio = fft_high_freq_ratio(&gray, 10, 10);
        assert!(ratio < 0.01);
    }

    #[test]
    fn test_tenengrad_uniform() {
        let gray = vec![128.0f32; 100];
        let t = tenengrad_energy(&gray, 10, 10);
        assert!(t < 0.001, "Uniform image should have ~0 tenengrad, got {}", t);
    }

    #[test]
    fn test_tenengrad_edge() {
        let mut gray = vec![0.0f32; 100];
        for y in 0..10 { for x in 5..10 { gray[y * 10 + x] = 255.0; } }
        let t = tenengrad_energy(&gray, 10, 10);
        assert!(t > 50.0, "Edge should have high tenengrad, got {}", t);
    }

    #[test]
    fn test_local_contrast_uniform() {
        let gray = vec![128.0f32; 64];
        let lc = local_contrast(&gray, 8, 8);
        assert!(lc < 0.001, "Uniform should have ~0 local contrast, got {}", lc);
    }

    #[test]
    fn test_local_contrast_checkerboard() {
        let mut gray = vec![0.0f32; 64];
        for y in 0..8 { for x in 0..8 { gray[y * 8 + x] = if (x + y) % 2 == 0 { 255.0 } else { 0.0 }; } }
        let lc = local_contrast(&gray, 8, 8);
        assert!(lc > 50.0, "Checkerboard should have high local contrast, got {}", lc);
    }

    #[test]
    fn test_blur_score_sharp_vs_blurry() {
        let mut sharp = vec![0.0f32; 100];
        for y in 0..10 { for x in 0..10 { sharp[y * 10 + x] = if (x + y) % 2 == 0 { 255.0 } else { 0.0 }; } }
        let blurry = vec![128.0f32; 100];
        let s = compute_blur_score(&sharp, 10, 10);
        let b = compute_blur_score(&blurry, 10, 10);
        assert!(s.combined > b.combined,
            "Sharp ({}) should score higher than blurry ({})", s.combined, b.combined);
    }

    #[test]
    fn test_blur_score_gradient_vs_uniform() {
        // Smooth gradient (slightly blurry look) vs uniform (completely flat)
        let mut gradient = vec![0.0f32; 100];
        for y in 0..10 { for x in 0..10 { gradient[y * 10 + x] = (x as f32) * 25.5; } }
        let uniform = vec![128.0f32; 100];
        let g = compute_blur_score(&gradient, 10, 10);
        let u = compute_blur_score(&uniform, 10, 10);
        assert!(g.combined > u.combined,
            "Gradient ({}) should score higher than uniform ({})", g.combined, u.combined);
    }

    #[test]
    fn test_is_blurry_thresholds() {
        assert!(is_blurry(30.0, Sensitivity::Strict));
        assert!(!is_blurry(50.0, Sensitivity::Strict));
        assert!(is_blurry(50.0, Sensitivity::Standard));
        assert!(!is_blurry(70.0, Sensitivity::Standard));
        assert!(is_blurry(70.0, Sensitivity::Lenient));
        assert!(!is_blurry(80.0, Sensitivity::Lenient));
    }

    #[test]
    fn test_small_image() {
        let gray = vec![128.0f32; 4];
        let scores = compute_blur_score(&gray, 2, 2);
        assert_eq!(scores.combined, 0.0);
    }

    #[test]
    fn test_downsample2x() {
        let gray: Vec<f32> = (0..16).map(|v| v as f32).collect();
        let (out, w, h) = downsample2x(&gray, 4, 4);
        assert_eq!(w, 2);
        assert_eq!(h, 2);
        assert_eq!(out.len(), 4);
        // top-left block: (0+1+4+5)/4 = 2.5
        assert!((out[0] - 2.5).abs() < 0.01);
    }

    #[test]
    fn test_multiscale_laplacian() {
        // Sharp checkerboard at 16x16 - multi-scale should still detect sharpness
        let mut gray = vec![0.0f32; 256];
        for y in 0..16 { for x in 0..16 { gray[y * 16 + x] = if (x + y) % 2 == 0 { 255.0 } else { 0.0 }; } }
        let scores = compute_blur_score(&gray, 16, 16);
        assert!(scores.combined > 20.0, "Checkerboard should have decent score, got {}", scores.combined);
        assert!(scores.laplacian_var > 0.0);
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;

    /// Write a grayscale buffer as a lossless PNG so tests are deterministic
    /// (no JPEG round-trip artifacts).
    fn write_png(path: &std::path::Path, gray: &[u8], w: u32, h: u32) {
        let img = image::ImageBuffer::<image::Luma<u8>, Vec<u8>>::from_raw(w, h, gray.to_vec())
            .expect("valid buffer");
        image::DynamicImage::ImageLuma8(img).save(path).expect("save png");
    }

    /// A block pattern (not a 1px checkerboard). A 1px checkerboard sits at the
    /// Nyquist frequency where 3-tap Sobel operators cancel out, so it scores
    /// poorly despite being "sharp". 4px blocks have strong edges at the scales
    /// the Laplacian/Sobel/Tenengrad metrics actually measure, like a real photo.
    fn checkerboard(w: u32, h: u32) -> Vec<u8> {
        let block = 4u32;
        let mut g = vec![0u8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                g[(y * w + x) as usize] = if (x / block + y / block) % 2 == 0 { 255 } else { 0 };
            }
        }
        g
    }

    #[test]
    fn test_batch_parallel_analyzes_all_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for i in 0..3 {
            let p = dir.path().join(format!("sharp_{i}.png"));
            write_png(&p, &checkerboard(64, 64), 64, 64);
            paths.push(p.to_string_lossy().to_string());
        }
        for i in 0..2 {
            let p = dir.path().join(format!("blur_{i}.png"));
            write_png(&p, &vec![128u8; 64 * 64], 64, 64);
            paths.push(p.to_string_lossy().to_string());
        }
        let results = analyze_batch_parallel(&paths, Sensitivity::Standard, false);
        assert_eq!(results.len(), 5, "all 5 files should be analyzed");
        for r in &results {
            assert!(paths.contains(&r.file_path), "result path must be an input");
        }
    }

    #[test]
    fn test_batch_parallel_sharp_beats_blurry() {
        let dir = tempfile::tempdir().unwrap();
        let sharp_p = dir.path().join("sharp.png");
        write_png(&sharp_p, &checkerboard(64, 64), 64, 64);
        let blur_p = dir.path().join("blur.png");
        write_png(&blur_p, &vec![128u8; 64 * 64], 64, 64);
        let paths = vec![
            sharp_p.to_string_lossy().to_string(),
            blur_p.to_string_lossy().to_string(),
        ];
        let results = analyze_batch_parallel(&paths, Sensitivity::Standard, false);
        let sharp = results.iter().find(|r| r.file_path.ends_with("sharp.png")).unwrap();
        let blur = results.iter().find(|r| r.file_path.ends_with("blur.png")).unwrap();
        assert!(sharp.score > blur.score, "sharp {} should exceed blurry {}", sharp.score, blur.score);
        assert!(!sharp.is_blurry, "sharp score {} should not be blurry", sharp.score);
        assert!(blur.is_blurry);
    }

    #[test]
    fn test_batch_parallel_accelerated_path() {
        let dir = tempfile::tempdir().unwrap();
        let sharp_p = dir.path().join("s.png");
        write_png(&sharp_p, &checkerboard(64, 64), 64, 64);
        let blur_p = dir.path().join("b.png");
        write_png(&blur_p, &vec![128u8; 64 * 64], 64, 64);
        let paths = vec![
            sharp_p.to_string_lossy().to_string(),
            blur_p.to_string_lossy().to_string(),
        ];
        let results = analyze_batch_parallel(&paths, Sensitivity::Standard, true);
        assert_eq!(results.len(), 2);
        let sharp = results.iter().find(|r| r.file_path.ends_with("s.png")).unwrap();
        let blur = results.iter().find(|r| r.file_path.ends_with("b.png")).unwrap();
        assert!(sharp.score > blur.score, "accelerated path must keep sharp>blurry ordering");
    }

    #[test]
    fn test_batch_parallel_empty_input() {
        let results = analyze_batch_parallel(&[], Sensitivity::Standard, false);
        assert!(results.is_empty());
    }

    #[test]
    fn test_batch_parallel_skips_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good.png");
        write_png(&good, &checkerboard(32, 32), 32, 32);
        let paths = vec![
            good.to_string_lossy().to_string(),
            dir.path().join("missing.png").to_string_lossy().to_string(),
        ];
        let results = analyze_batch_parallel(&paths, Sensitivity::Standard, false);
        assert_eq!(results.len(), 1, "missing file should be skipped, not crash");
    }

    #[test]
    fn test_analyze_file_with_options_consistency() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("img.png");
        write_png(&p, &checkerboard(64, 64), 64, 64);
        let path = p.to_string_lossy().to_string();
        let normal = analyze_file_with_options(&path, Sensitivity::Standard, false).unwrap();
        let fast = analyze_file_with_options(&path, Sensitivity::Standard, true).unwrap();
        // Both paths should agree this checkerboard is sharp (well above threshold).
        assert!(normal.score > 60.0, "normal score {}", normal.score);
        assert!(fast.score > 60.0, "fast score {}", fast.score);
    }
}


#[cfg(test)]
mod bench_tests {
    use super::*;


    /// Env-gated benchmark (NOT a pass/fail test): set NPS_BENCH_DIR to a folder
    /// of NEFs to measure blur-detection throughput on real RAW files.
    ///
    /// It quantifies the three things the optimization targets:
    ///   1. multi-core parallel speedup  (serial vs the parallel work-stealing pool)
    ///   2. the analysis-gray disk cache  (cold first run vs warm re-run; the warm
    ///      run skips the expensive NEF-preview JPEG decode entirely)
    ///   3. the accelerated toggle        (standard vs accelerated buffer; the
    ///      accelerated path is genuinely smaller + fused, so it is measurably faster)
    ///
    /// Skips silently when NPS_BENCH_DIR is unset.
    #[test]
    fn bench_blur_parallel() {
        let dir = match std::env::var("NPS_BENCH_DIR") {
            Ok(d) if !d.is_empty() => d,
            _ => {
                eprintln!("NPS_BENCH_DIR not set; skipping blur benchmark");
                return;
            }
        };
        let limit: usize = std::env::var("NPS_BENCH_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
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

        let threads = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(0)
            .min(n)
            .min(12)
            .max(1);
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;

        // Pass 1: cold analysis-gray cache. The analysis gray buffer is decoded
        // (NEF preview JPEG) and written to disk here for the first time this run.
        let c0 = std::time::Instant::now();
        let cold = analyze_batch_parallel(&paths, Sensitivity::Standard, false);
        let cold_dur = c0.elapsed();

        // Pass 2: warm analysis-gray cache -> the JPEG decode is skipped entirely.
        let w0 = std::time::Instant::now();
        let warm = analyze_batch_parallel(&paths, Sensitivity::Standard, false);
        let warm_dur = w0.elapsed();

        // Serial (warm cache) to quantify the multi-core speedup.
        let s0 = std::time::Instant::now();
        let serial: Vec<BlurResult> = paths
            .iter()
            .filter_map(|p| analyze_file_with_options(p, Sensitivity::Standard, false).ok())
            .collect();
        let serial_dur = s0.elapsed();

        // Accelerated path: its (smaller) analysis buffer has its own cache entry,
        // so warm it up untimed first, then time the steady-state warm pass. This is
        // the genuinely-faster fast path (smaller buffer + fused gradient pass).
        let _ = analyze_batch_parallel(&paths, Sensitivity::Standard, true);
        let a0 = std::time::Instant::now();
        let acc = analyze_batch_parallel(&paths, Sensitivity::Standard, true);
        let acc_dur = a0.elapsed();

        eprintln!("=== BLUR DETECTION BENCH ({n} NEFs, {threads} worker threads) ===");
        eprintln!("parallel standard   (cold cache): {:?} ({:.1} ms/file)", cold_dur, ms(cold_dur) / n as f64);
        eprintln!("parallel standard   (warm cache): {:?} ({:.1} ms/file)  [cache skips JPEG decode]", warm_dur, ms(warm_dur) / n as f64);
        eprintln!("serial   standard   (warm cache): {:?} ({:.1} ms/file)", serial_dur, ms(serial_dur) / n as f64);
        eprintln!("  -> parallel speedup vs serial  : {:.2}x", ms(serial_dur) / ms(warm_dur).max(1e-9));
        eprintln!("parallel accelerated (warm cache): {:?} ({:.1} ms/file)  [{:.2}x vs serial]", acc_dur, ms(acc_dur) / n as f64, ms(serial_dur) / ms(acc_dur).max(1e-9));
        eprintln!("scored: cold={} warm={} serial={} accelerated={}", cold.len(), warm.len(), serial.len(), acc.len());

        // Per-file breakdown: raw decode (no cache) vs cached gray read vs the two
        // compute sizes, so we can see exactly where the time goes and why the
        // accelerated path is faster.
        let probe = n.min(12);
        let mut decode_ms = 0.0f64;
        let mut cached_ms = 0.0f64;
        let mut score_std_ms = 0.0f64;
        let mut score_acc_ms = 0.0f64;
        let mut dims_std = String::new();
        let mut dims_acc = String::new();
        for p in paths.iter().take(probe) {
            let d0 = std::time::Instant::now();
            let (gray, w, h) = load_analysis_gray(p, ANALYSIS_DIM).expect("load gray");
            decode_ms += d0.elapsed().as_secs_f64() * 1000.0;
            let r0 = std::time::Instant::now();
            let cached = crate::library::commands::analysis_cache_read(p, ANALYSIS_DIM);
            cached_ms += r0.elapsed().as_secs_f64() * 1000.0;
            let t0 = std::time::Instant::now();
            let _ = compute_blur_score(&gray, w, h);
            score_std_ms += t0.elapsed().as_secs_f64() * 1000.0;
            if dims_std.is_empty() { dims_std = format!("{w}x{h}"); }
            let (ag, aw, ah) = load_analysis_gray(p, ANALYSIS_DIM_FAST).expect("load gray fast");
            let t1 = std::time::Instant::now();
            let _ = compute_blur_score(&ag, aw, ah);
            score_acc_ms += t1.elapsed().as_secs_f64() * 1000.0;
            if dims_acc.is_empty() { dims_acc = format!("{aw}x{ah}"); }
            let _ = cached;
        }
        eprintln!("breakdown (avg of {probe}):");
        eprintln!("  raw decode+gray  std {dims_std} (first-run floor): {:.1} ms", decode_ms / probe as f64);
        eprintln!("  cached gray read (re-run, skips decode)          : {:.2} ms", cached_ms / probe as f64);
        eprintln!("  metric compute   std {dims_std}                   : {:.1} ms", score_std_ms / probe as f64);
        eprintln!("  metric compute   accel {dims_acc}                 : {:.1} ms", score_acc_ms / probe as f64);
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    /// Build a sharp high-contrast checkerboard PNG on disk and return its path.
    fn write_sharp_png(path: &std::path::Path, w: u32, h: u32, cell: u32) {
        let mut img = image::GrayImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = if ((x / cell) + (y / cell)) % 2 == 0 { 235u8 } else { 20u8 };
                img.put_pixel(x, y, image::Luma([v]));
            }
        }
        img.save(path).unwrap();
    }

    /// The on-disk analysis-gray cache stores integer 0-255 pixels and the loader
    /// quantizes before scoring, so a cached re-run must reproduce exactly the same
    /// blur score as the cold decode. This guards the lossless round-trip the
    /// speed-up relies on (standard path: no resize, native preview resolution).
    #[test]
    fn analysis_cache_roundtrip_is_lossless() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pattern.png");
        write_sharp_png(&path, 96, 64, 4);
        let path_str = path.to_str().unwrap().to_string();

        // Cold: decodes the PNG and writes the analysis cache.
        let cold = analyze_file_with_options(&path_str, Sensitivity::Standard, false).unwrap();
        // The cache must now hold a buffer for this file.
        assert!(
            crate::library::commands::analysis_cache_read(&path_str, ANALYSIS_DIM).is_some(),
            "cache should be populated after the first analysis"
        );
        // Warm: reads the cached gray buffer (skips the decode).
        let warm = analyze_file_with_options(&path_str, Sensitivity::Standard, false).unwrap();

        assert_eq!(cold.score.to_bits(), warm.score.to_bits(), "score must survive the cache round-trip");
        assert_eq!(cold.laplacian_var.to_bits(), warm.laplacian_var.to_bits());
        assert_eq!(cold.gradient_energy.to_bits(), warm.gradient_energy.to_bits());
        assert_eq!(cold.is_blurry, warm.is_blurry);
        assert!(cold.score > 60.0, "sharp pattern should score high, got {}", cold.score);
    }

    /// The accelerated path downscales to a smaller buffer (exercising the resize +
    /// quantize step) and uses its own cache entry; verify it too round-trips
    /// losslessly and still rates a sharp pattern as sharp.
    #[test]
    fn analysis_cache_roundtrip_accelerated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pattern_acc.png");
        // Larger than ANALYSIS_DIM_FAST so the accelerated path actually downscales.
        write_sharp_png(&path, 640, 480, 6);
        let path_str = path.to_str().unwrap().to_string();

        let cold = analyze_file_with_options(&path_str, Sensitivity::Standard, true).unwrap();
        assert!(
            crate::library::commands::analysis_cache_read(&path_str, ANALYSIS_DIM_FAST).is_some(),
            "accelerated cache should be populated after the first analysis"
        );
        let warm = analyze_file_with_options(&path_str, Sensitivity::Standard, true).unwrap();
        assert_eq!(cold.score.to_bits(), warm.score.to_bits(), "fast-path score must survive the round-trip");
        assert!(cold.score > 60.0, "sharp pattern should score high on the fast path, got {}", cold.score);
    }

    /// A uniform (blurry) image must round-trip to the same low score on both paths.
    #[test]
    fn analysis_cache_roundtrip_blurry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flat.png");
        let img = image::GrayImage::from_pixel(120, 80, image::Luma([128u8]));
        img.save(&path).unwrap();
        let path_str = path.to_str().unwrap().to_string();

        let cold = analyze_file_with_options(&path_str, Sensitivity::Standard, false).unwrap();
        let warm = analyze_file_with_options(&path_str, Sensitivity::Standard, false).unwrap();
        assert_eq!(cold.score.to_bits(), warm.score.to_bits());
        assert!(cold.score < 40.0, "uniform image should score low, got {}", cold.score);
        assert!(cold.is_blurry);
    }
}
