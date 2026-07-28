//! Offline machine-learning photo classification.
//!
//! UltraFace detects faces and SqueezeNet 1.1 supplies ImageNet semantic
//! predictions. Both ONNX models are embedded in the executable and evaluated
//! by tract, so classification works without Python, a network connection, or
//! uploading private photos.

use super::{ClassifyResult, PhotoCategory};
use image::{imageops::FilterType, DynamicImage};
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use tract_onnx::prelude::*;

const FACE_MODEL: &[u8] = include_bytes!("../../../models/version-RFB-320.onnx");
const SCENE_MODEL: &[u8] = include_bytes!("../../../models/squeezenet1.1-7.onnx");

type Runnable = SimplePlan<TypedFact, Box<dyn TypedOp>, TypedModel>;

/// ImageNet classes that strongly indicate an outdoor/natural scene. The
/// classifier considers the top five predictions, not just the top one.
const LANDSCAPE_CLASSES: &[usize] = &[
    562, // fountain
    576, // gondola
    672, // mountain tent
    795, // ski
    802, // snowmobile
    821, // steel arch bridge
    829, // streetcar
    970, // alp
    972, // cliff
    973, // coral reef
    974, // geyser
    975, // lakeside
    976, // promontory
    977, // sandbar
    978, // seashore
    979, // valley
    980, // volcano
];

pub struct Classifier {
    // tract plans contain internal scratch state, so each model is guarded
    // while batch workers continue decoding/preprocessing images in parallel.
    face: Arc<Mutex<Runnable>>,
    scene: Arc<Mutex<Runnable>>,
}

impl Classifier {
    pub fn new(_model_dir: &str) -> Result<Self, String> {
        let face = load_model(FACE_MODEL, "UltraFace")?;
        let scene = load_model(SCENE_MODEL, "SqueezeNet")?;
        Ok(Self {
            face: Arc::new(Mutex::new(face)),
            scene: Arc::new(Mutex::new(scene)),
        })
    }

    pub fn classify_file(&self, path: &str) -> Result<ClassifyResult, String> {
        let img = crate::library::commands::load_oriented_image(path)?;

        let face_confidence = self.detect_face(&img)?;
        if face_confidence >= 0.62 {
            return Ok(ClassifyResult {
                file_path: path.to_string(),
                category: PhotoCategory::Portrait,
                confidence: face_confidence,
                has_person: true,
            });
        }

        let (top5, scene_confidence) = self.classify_semantics(&img)?;
        let landscape_probability: f32 = top5
            .iter()
            .filter(|(class_id, _)| LANDSCAPE_CLASSES.contains(class_id))
            .map(|(_, probability)| *probability)
            .sum();
        let is_landscape = landscape_probability >= 0.16
            || top5.first().map(|(id, _)| LANDSCAPE_CLASSES.contains(id)).unwrap_or(false);

        Ok(ClassifyResult {
            file_path: path.to_string(),
            category: if is_landscape { PhotoCategory::Landscape } else { PhotoCategory::Other },
            confidence: if is_landscape {
                landscape_probability.max(scene_confidence)
            } else {
                scene_confidence
            },
            has_person: false,
        })
    }

    fn detect_face(&self, img: &DynamicImage) -> Result<f32, String> {
        let rgb = img.resize_exact(320, 240, FilterType::Triangle).to_rgb8();
        let mut input = vec![0f32; 1 * 3 * 240 * 320];
        let plane = 240 * 320;
        for (i, p) in rgb.pixels().enumerate() {
            input[i] = (p[0] as f32 - 127.0) / 128.0;
            input[plane + i] = (p[1] as f32 - 127.0) / 128.0;
            input[2 * plane + i] = (p[2] as f32 - 127.0) / 128.0;
        }
        let tensor = Tensor::from_shape(&[1, 3, 240, 320], &input)
            .map_err(|e| format!("face input failed: {e}"))?;
        let outputs = self.face.lock().map_err(|_| "face model lock poisoned".to_string())?
            .run(tvec!(tensor.into()))
            .map_err(|e| format!("face inference failed: {e}"))?;

        // UltraFace outputs boxes [1,4420,4] and scores [1,4420,2]. Find the
        // latter by shape so exporter output ordering does not matter.
        for output in outputs.iter() {
            let shape = output.shape();
            if shape.last() == Some(&2) {
                let scores = output.to_array_view::<f32>()
                    .map_err(|e| format!("face scores failed: {e}"))?;
                let flat: Vec<f32> = scores.iter().copied().collect();
                return Ok(flat.chunks_exact(2).map(|v| v[1]).fold(0.0f32, f32::max));
            }
        }
        Err("UltraFace score output missing".to_string())
    }

    fn classify_semantics(&self, img: &DynamicImage) -> Result<(Vec<(usize, f32)>, f32), String> {
        let rgb = img.resize_exact(224, 224, FilterType::Lanczos3).to_rgb8();
        let means = [0.485f32, 0.456, 0.406];
        let stds = [0.229f32, 0.224, 0.225];
        let plane = 224 * 224;
        let mut input = vec![0f32; 3 * plane];
        for (i, p) in rgb.pixels().enumerate() {
            for c in 0..3 {
                input[c * plane + i] = (p[c] as f32 / 255.0 - means[c]) / stds[c];
            }
        }
        let tensor = Tensor::from_shape(&[1, 3, 224, 224], &input)
            .map_err(|e| format!("scene input failed: {e}"))?;
        let outputs = self.scene.lock().map_err(|_| "scene model lock poisoned".to_string())?
            .run(tvec!(tensor.into()))
            .map_err(|e| format!("scene inference failed: {e}"))?;
        let logits: Vec<f32> = outputs[0]
            .to_array_view::<f32>()
            .map_err(|e| format!("scene output failed: {e}"))?
            .iter().copied().collect();
        if logits.len() < 1000 {
            return Err(format!("unexpected SqueezeNet output: {} values", logits.len()));
        }
        let max_logit = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let exp_sum: f32 = logits.iter().map(|v| (*v - max_logit).exp()).sum();
        let mut ranked: Vec<(usize, f32)> = logits.iter().enumerate()
            .map(|(i, v)| (i, (*v - max_logit).exp() / exp_sum.max(f32::EPSILON)))
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        ranked.truncate(5);
        let confidence = ranked.first().map(|(_, p)| *p).unwrap_or(0.0);
        Ok((ranked, confidence))
    }

    pub fn classify_batch(&self, paths: &[String]) -> Vec<ClassifyResult> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let n = paths.len();
        if n == 0 { return Vec::new(); }
        let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(6).min(n);
        let next = AtomicUsize::new(0);
        let out = Mutex::new(Vec::with_capacity(n));
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= n { break; }
                    let result = self.classify_file(&paths[index]).unwrap_or_else(|e| {
                        tracing::warn!("classification failed for {}: {e}", paths[index]);
                        ClassifyResult {
                            file_path: paths[index].clone(),
                            category: PhotoCategory::Other,
                            confidence: 0.0,
                            has_person: false,
                        }
                    });
                    out.lock().expect("classification result lock").push((index, result));
                });
            }
        });
        let mut values = out.into_inner().unwrap_or_default();
        values.sort_by_key(|(index, _)| *index);
        values.into_iter().map(|(_, result)| result).collect()
    }
}

fn load_model(bytes: &'static [u8], name: &str) -> Result<Runnable, String> {
    tract_onnx::onnx()
        .model_for_read(&mut Cursor::new(bytes))
        .map_err(|e| format!("load {name} model failed: {e}"))?
        .into_optimized()
        .map_err(|e| format!("optimise {name} model failed: {e}"))?
        .into_runnable()
        .map_err(|e| format!("prepare {name} model failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_models_load() {
        Classifier::new("").expect("embedded ONNX models should compile");
    }

    #[test]
    fn landscape_mapping_contains_core_scene_classes() {
        assert!(LANDSCAPE_CLASSES.contains(&970)); // alp
        assert!(LANDSCAPE_CLASSES.contains(&978)); // seashore
        assert!(!LANDSCAPE_CLASSES.contains(&0));
    }

    #[test]
    fn inference_pipeline_runs_on_a_real_image_buffer() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("gradient.jpg");
        let image = image::RgbImage::from_fn(320, 240, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
        });
        image.save(&path).expect("save test image");
        let result = Classifier::new("")
            .expect("models load")
            .classify_file(path.to_str().expect("utf8 path"))
            .expect("both ONNX models should run");
        assert!(result.confidence.is_finite());
        assert!((0.0..=1.0).contains(&result.confidence));
    }
}
