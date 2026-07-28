pub mod commands;
pub mod blur;
pub mod classify;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlurResult {
    pub file_path: String,
    pub score: f32,
    pub laplacian_var: f32,
    pub gradient_energy: f32,
    pub fft_hf_ratio: f32,
    pub is_blurry: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PhotoCategory {
    Portrait,
    Landscape,
    Other,
}

impl std::fmt::Display for PhotoCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PhotoCategory::Portrait => write!(f, "portrait"),
            PhotoCategory::Landscape => write!(f, "landscape"),
            PhotoCategory::Other => write!(f, "other"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyResult {
    pub file_path: String,
    pub category: PhotoCategory,
    pub confidence: f32,
    pub has_person: bool,
}

/// Blur detection sensitivity levels
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Sensitivity {
    Strict,   // threshold > 40
    Standard, // threshold > 60
    Lenient,  // threshold > 75
}

impl Sensitivity {
    pub fn threshold(&self) -> f32 {
        match self {
            Sensitivity::Strict => 40.0,
            Sensitivity::Standard => 60.0,
            Sensitivity::Lenient => 75.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensitivity_thresholds() {
        assert_eq!(Sensitivity::Strict.threshold(), 40.0);
        assert_eq!(Sensitivity::Standard.threshold(), 60.0);
        assert_eq!(Sensitivity::Lenient.threshold(), 75.0);
    }

    #[test]
    fn test_category_display() {
        assert_eq!(PhotoCategory::Portrait.to_string(), "portrait");
        assert_eq!(PhotoCategory::Landscape.to_string(), "landscape");
        assert_eq!(PhotoCategory::Other.to_string(), "other");
    }
}
