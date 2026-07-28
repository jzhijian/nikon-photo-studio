pub mod scanner;
pub mod archive;
pub mod recycle;

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub filename: String,
    pub extension: String,
    pub size: u64,
    pub modified: Option<String>,
}

/// Check if a file is a supported image format
pub fn is_supported_image(path: &str) -> bool {
    let extensions = ["nef", "jpg", "jpeg", "tif", "tiff", "png"];
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| extensions.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Get the sidecar (.nps) path for a given image file
pub fn sidecar_path(image_path: &str) -> String {
    format!("{}.nps", image_path)
}

/// Check if a sidecar file exists for the given image
pub fn has_sidecar(image_path: &str) -> bool {
    Path::new(&sidecar_path(image_path)).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported_image() {
        assert!(is_supported_image("photo.nef"));
        assert!(is_supported_image("photo.NEF"));
        assert!(is_supported_image("photo.jpg"));
        assert!(is_supported_image("photo.jpeg"));
        assert!(is_supported_image("photo.tif"));
        assert!(is_supported_image("photo.png"));
        assert!(!is_supported_image("video.mp4"));
        assert!(!is_supported_image("doc.pdf"));
        assert!(!is_supported_image("noext"));
    }

    #[test]
    fn test_sidecar_path() {
        assert_eq!(sidecar_path("D:/photos/DSC_001.NEF"), "D:/photos/DSC_001.NEF.nps");
    }
}
