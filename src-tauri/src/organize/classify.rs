//! Offline portrait detection powered by the embedded UltraFace ONNX model.
//! No scene classifier is used: the Organize module intentionally exposes only
//! blur results, portraits, and other photos.

use super::{ClassifyResult, PhotoCategory};
use image::{imageops::FilterType, DynamicImage};
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use tract_onnx::prelude::*;

const FACE_MODEL: &[u8] = include_bytes!("../../../models/version-RFB-320.onnx");
type Runnable = SimplePlan<TypedFact, Box<dyn TypedOp>, TypedModel>;

pub struct Classifier {
    face: Arc<Mutex<Runnable>>,
}

impl Classifier {
    pub fn new() -> Result<Self, String> {
        let face = tract_onnx::onnx()
            .model_for_read(&mut Cursor::new(FACE_MODEL))
            .map_err(|e| format!("load UltraFace model failed: {e}"))?
            .into_optimized()
            .map_err(|e| format!("optimise UltraFace model failed: {e}"))?
            .into_runnable()
            .map_err(|e| format!("prepare UltraFace model failed: {e}"))?;
        Ok(Self { face: Arc::new(Mutex::new(face)) })
    }

    pub fn detect_file(&self, path: &str, threshold: f32) -> Result<ClassifyResult, String> {
        let img = crate::library::commands::load_oriented_image(path)?;
        let confidence = self.detect_face(&img)?;
        let limit = threshold.clamp(0.05, 0.99);
        let is_portrait = confidence >= limit;
        Ok(ClassifyResult {
            file_path: path.to_string(),
            category: if is_portrait { PhotoCategory::Portrait } else { PhotoCategory::Other },
            confidence,
            has_person: is_portrait,
        })
    }

    fn detect_face(&self, img: &DynamicImage) -> Result<f32, String> {
        let rgb = img.resize_exact(320, 240, FilterType::Triangle).to_rgb8();
        let plane = 240 * 320;
        let mut input = vec![0f32; 3 * plane];
        for (i, p) in rgb.pixels().enumerate() {
            input[i] = (p[0] as f32 - 127.0) / 128.0;
            input[plane + i] = (p[1] as f32 - 127.0) / 128.0;
            input[2 * plane + i] = (p[2] as f32 - 127.0) / 128.0;
        }
        let tensor = Tensor::from_shape(&[1, 3, 240, 320], &input)
            .map_err(|e| format!("portrait input failed: {e}"))?;
        let outputs = self.face.lock().map_err(|_| "portrait model lock poisoned".to_string())?
            .run(tvec!(tensor.into()))
            .map_err(|e| format!("portrait inference failed: {e}"))?;
        for output in outputs.iter() {
            if output.shape().last() == Some(&2) {
                let scores = output.to_array_view::<f32>()
                    .map_err(|e| format!("portrait scores failed: {e}"))?;
                let flat: Vec<f32> = scores.iter().copied().collect();
                return Ok(flat.chunks_exact(2).map(|v| v[1]).fold(0.0f32, f32::max));
            }
        }
        Err("UltraFace score output missing".to_string())
    }

    pub fn detect_batch(&self, paths: &[String], threshold: f32) -> Vec<ClassifyResult> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let n = paths.len();
        if n == 0 { return Vec::new(); }
        let workers = std::thread::available_parallelism().map(|v| v.get()).unwrap_or(4).min(6).min(n);
        let next = AtomicUsize::new(0);
        let out = Mutex::new(Vec::with_capacity(n));
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= n { break; }
                    let result = self.detect_file(&paths[index], threshold).unwrap_or_else(|e| {
                        tracing::warn!("portrait detection failed for {}: {e}", paths[index]);
                        ClassifyResult {
                            file_path: paths[index].clone(),
                            category: PhotoCategory::Other,
                            confidence: 0.0,
                            has_person: false,
                        }
                    });
                    out.lock().expect("portrait result lock").push((index, result));
                });
            }
        });
        let mut values = out.into_inner().unwrap_or_default();
        values.sort_by_key(|(index, _)| *index);
        values.into_iter().map(|(_, result)| result).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_portrait_model_loads() {
        Classifier::new().expect("embedded UltraFace model should compile");
    }

    #[test]
    fn inference_returns_linear_confidence() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("gradient.jpg");
        image::RgbImage::from_fn(320, 240, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
        }).save(&path).expect("save test image");
        let detector = Classifier::new().expect("model loads");
        let result = detector.detect_file(path.to_str().unwrap(), 0.62).expect("inference runs");
        assert!((0.0..=1.0).contains(&result.confidence));
    }
}
