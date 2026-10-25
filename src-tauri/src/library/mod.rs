pub mod commands;

pub mod nikon_sdk;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoRecord {
    pub id: i64,
    pub file_path: String,
    pub filename: String,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub file_size: u64,
    pub date_taken: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub aperture: Option<f32>,
    pub shutter_speed: Option<String>,
    pub iso: Option<u32>,
    pub focal_length: Option<f32>,
    pub blur_score: Option<f32>,
    pub category: Option<String>,
    pub is_deleted: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExifData {
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub aperture: Option<f32>,
    pub shutter_speed: Option<String>,
    pub iso: Option<u32>,
    pub focal_length: Option<f32>,
    pub white_balance: Option<String>,
    pub date_taken: Option<String>,
    pub width: u32,
    pub height: u32,
}

/// Scan a directory for image files (NEF, JPG, TIFF)
pub fn scan_directory(path: &str) -> Vec<String> {
    let extensions = ["nef", "jpg", "jpeg", "tif", "tiff"];
    let mut files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    if extensions.contains(&ext.to_lowercase().as_str()) {
                        files.push(p.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_scan_directory_finds_images() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("photo1.nef"), b"fake").unwrap();
        fs::write(dir.path().join("photo2.jpg"), b"fake").unwrap();
        fs::write(dir.path().join("photo3.NEF"), b"fake").unwrap();
        fs::write(dir.path().join("readme.txt"), b"fake").unwrap();
        fs::write(dir.path().join("video.mp4"), b"fake").unwrap();

        let files = scan_directory(dir.path().to_str().unwrap());
        assert_eq!(files.len(), 3);
        assert!(files.iter().all(|f| {
            let lower = f.to_lowercase();
            lower.ends_with(".nef") || lower.ends_with(".jpg")
        }));
    }

    #[test]
    fn test_scan_directory_empty() {
        let dir = tempfile::tempdir().unwrap();
        let files = scan_directory(dir.path().to_str().unwrap());
        assert!(files.is_empty());
    }

    #[test]
    fn test_scan_directory_nonexistent() {
        let files = scan_directory("/nonexistent/path/xyz");
        assert!(files.is_empty());
    }
}
