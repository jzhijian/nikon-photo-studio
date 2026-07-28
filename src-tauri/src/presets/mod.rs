pub mod commands;
pub mod parser;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub id: i64,
    pub name: String,
    pub format: PresetFormat,
    pub params: PresetParams,
    pub lut_path: Option<String>,
    pub source: PresetSource,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PresetFormat {
    XMP,
    Cube,
    NP3,
    NCP,
    LRPreset,
    ThreeDL,
    COStyle,
}

impl std::fmt::Display for PresetFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PresetFormat::XMP => write!(f, "xmp"),
            PresetFormat::Cube => write!(f, "cube"),
            PresetFormat::NP3 => write!(f, "np3"),
            PresetFormat::NCP => write!(f, "ncp"),
            PresetFormat::LRPreset => write!(f, "lrpreset"),
            PresetFormat::ThreeDL => write!(f, "3dl"),
            PresetFormat::COStyle => write!(f, "costyle"),
        }
    }
}

impl PresetFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "xmp" => Some(PresetFormat::XMP),
            "cube" => Some(PresetFormat::Cube),
            "np3" => Some(PresetFormat::NP3),
            "ncp" => Some(PresetFormat::NCP),
            "lrpreset" => Some(PresetFormat::LRPreset),
            "3dl" => Some(PresetFormat::ThreeDL),
            "costyle" => Some(PresetFormat::COStyle),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PresetSource {
    Builtin,
    Imported,
    NikonCloud,
    UserCreated,
}

/// Develop parameters that can be applied to an image
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PresetParams {
    pub exposure: Option<f32>,
    pub contrast: Option<f32>,
    pub highlights: Option<f32>,
    pub shadows: Option<f32>,
    pub whites: Option<f32>,
    pub blacks: Option<f32>,
    pub temperature: Option<f32>,
    pub tint: Option<f32>,
    pub vibrance: Option<f32>,
    pub saturation: Option<f32>,
    pub clarity: Option<f32>,
    pub sharpening: Option<f32>,
    // Tone curve points
    pub tone_curve: Option<Vec<(f32, f32)>>,
    // HSL adjustments
    pub hue_adjustments: Option<[f32; 8]>,
    pub saturation_adjustments: Option<[f32; 8]>,
    pub luminance_adjustments: Option<[f32; 8]>,
    // Split toning
    pub highlight_hue: Option<f32>,
    pub highlight_saturation: Option<f32>,
    pub shadow_hue: Option<f32>,
    pub shadow_saturation: Option<f32>,
}

impl PresetParams {
    /// Merge two param sets (self is base, other overrides)
    pub fn merge(&self, other: &PresetParams) -> PresetParams {
        PresetParams {
            exposure: other.exposure.or(self.exposure),
            contrast: other.contrast.or(self.contrast),
            highlights: other.highlights.or(self.highlights),
            shadows: other.shadows.or(self.shadows),
            whites: other.whites.or(self.whites),
            blacks: other.blacks.or(self.blacks),
            temperature: other.temperature.or(self.temperature),
            tint: other.tint.or(self.tint),
            vibrance: other.vibrance.or(self.vibrance),
            saturation: other.saturation.or(self.saturation),
            clarity: other.clarity.or(self.clarity),
            sharpening: other.sharpening.or(self.sharpening),
            tone_curve: other.tone_curve.clone().or_else(|| self.tone_curve.clone()),
            hue_adjustments: other.hue_adjustments.or(self.hue_adjustments),
            saturation_adjustments: other.saturation_adjustments.or(self.saturation_adjustments),
            luminance_adjustments: other.luminance_adjustments.or(self.luminance_adjustments),
            highlight_hue: other.highlight_hue.or(self.highlight_hue),
            highlight_saturation: other.highlight_saturation.or(self.highlight_saturation),
            shadow_hue: other.shadow_hue.or(self.shadow_hue),
            shadow_saturation: other.shadow_saturation.or(self.shadow_saturation),
        }
    }

    /// Apply params to a pixel (simplified - for full pipeline use libraw)
    pub fn apply_to_pixel(&self, r: f32, g: f32, b: f32) -> (f32, f32, f32) {
        let mut r = r;
        let mut g = g;
        let mut b = b;

        // Exposure (EV stops)
        if let Some(ev) = self.exposure {
            let factor = 2.0f32.powf(ev);
            r *= factor;
            g *= factor;
            b *= factor;
        }

        // Contrast (around 0.5 midpoint)
        if let Some(c) = self.contrast {
            let factor = 1.0 + c / 100.0;
            r = (r - 0.5) * factor + 0.5;
            g = (g - 0.5) * factor + 0.5;
            b = (b - 0.5) * factor + 0.5;
        }

        // Saturation
        if let Some(s) = self.saturation {
            let factor = 1.0 + s / 100.0;
            let luma = 0.299 * r + 0.587 * g + 0.114 * b;
            r = luma + (r - luma) * factor;
            g = luma + (g - luma) * factor;
            b = luma + (b - luma) * factor;
        }

        // Temperature (simple warm/cool shift)
        if let Some(t) = self.temperature {
            let shift = t / 100.0;
            r += shift * 0.1;
            b -= shift * 0.1;
        }

        // Clamp
        r = r.clamp(0.0, 1.0);
        g = g.clamp(0.0, 1.0);
        b = b.clamp(0.0, 1.0);

        (r, g, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_format_from_extension() {
        assert_eq!(PresetFormat::from_extension("xmp"), Some(PresetFormat::XMP));
        assert_eq!(PresetFormat::from_extension("XMP"), Some(PresetFormat::XMP));
        assert_eq!(PresetFormat::from_extension("cube"), Some(PresetFormat::Cube));
        assert_eq!(PresetFormat::from_extension("np3"), Some(PresetFormat::NP3));
        assert_eq!(PresetFormat::from_extension("ncp"), Some(PresetFormat::NCP));
        assert_eq!(PresetFormat::from_extension("lrpreset"), Some(PresetFormat::LRPreset));
        assert_eq!(PresetFormat::from_extension("3dl"), Some(PresetFormat::ThreeDL));
        assert_eq!(PresetFormat::from_extension("costyle"), Some(PresetFormat::COStyle));
        assert_eq!(PresetFormat::from_extension("jpg"), None);
    }

    #[test]
    fn test_params_merge() {
        let base = PresetParams {
            exposure: Some(0.5),
            contrast: Some(10.0),
            ..Default::default()
        };
        let overlay = PresetParams {
            exposure: Some(1.0),
            saturation: Some(20.0),
            ..Default::default()
        };
        let merged = base.merge(&overlay);
        assert_eq!(merged.exposure, Some(1.0)); // overridden
        assert_eq!(merged.contrast, Some(10.0)); // kept from base
        assert_eq!(merged.saturation, Some(20.0)); // from overlay
    }

    #[test]
    fn test_apply_exposure() {
        let params = PresetParams {
            exposure: Some(1.0), // +1 EV = double brightness
            ..Default::default()
        };
        let (r, g, b) = params.apply_to_pixel(0.25, 0.25, 0.25);
        assert!((r - 0.5).abs() < 0.01);
        assert!((g - 0.5).abs() < 0.01);
        assert!((b - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_apply_saturation() {
        let params = PresetParams {
            saturation: Some(-100.0), // full desaturation
            ..Default::default()
        };
        let (r, g, b) = params.apply_to_pixel(1.0, 0.0, 0.0);
        // Should be grayscale: r == g == b
        assert!((r - g).abs() < 0.01);
        assert!((g - b).abs() < 0.01);
    }

    #[test]
    fn test_apply_clamps() {
        let params = PresetParams {
            exposure: Some(5.0), // very high
            ..Default::default()
        };
        let (r, _, _) = params.apply_to_pixel(0.9, 0.9, 0.9);
        assert!(r <= 1.0);
    }
}
