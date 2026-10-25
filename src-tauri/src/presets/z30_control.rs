//! Codec for the documented Z30 transport dataset, not an NP3/NCP file parser.
//! Integer adjustment fields retain camera units; no rendering or compatibility
//! with Flexible Color is implied by a successful round trip.

use thiserror::Error;

pub const BASIC_SIZE: usize = 36;
pub const CURVE_SIZE: usize = 578;
pub const EXTENDED_SIZE: usize = BASIC_SIZE + CURVE_SIZE;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ControlError {
    #[error("Z30 dataset must contain 36 or 614 bytes, got {0}")]
    Length(usize),
    #[error("Unsupported Z30 base Picture Control: {0}")]
    UnsupportedBase(u16),
    #[error("Invalid Z30 Picture Control field: {0}")]
    Field(&'static str),
    #[error("An unused camera slot cannot be uploaded as a preset")]
    UnusedSlot,
    #[error("Custom Picture Control destination must be C1-C9 (201-209), got {0}")]
    Destination(u16),
}

/// Curve payloads are opaque camera data. Host headers and sample words are
/// preserved, not reconstructed from a displayed gamma label or control points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveData {
    pub enabled: bool,
    pub bytes: Option<Box<[u8; CURVE_SIZE]>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureControl {
    /// Zero denotes an unregistered slot. It is accepted for backup only.
    pub base: u16,
    pub registration_name: String,
    pub apply_level: u8,
    pub quick_sharp_enabled: bool,
    /// Whole units. This field does not use the quarter-step encoding below.
    pub quick_sharp: i8,
    pub sharpening_quarters: i8,
    pub middle_range_sharpening_quarters: i8,
    pub clarity_quarters: i8,
    pub contrast_quarters: i8,
    pub brightness_quarters: i8,
    pub saturation_quarters: i8,
    pub hue_quarters: i8,
    pub filter_effects: u8,
    pub toning: u8,
    pub toning_density_quarters: i8,
    pub curve: CurveData,
}

fn require(condition: bool, field: &'static str) -> Result<(), ControlError> {
    if condition {
        Ok(())
    } else {
        Err(ControlError::Field(field))
    }
}

impl PictureControl {
    pub fn decode(raw: &[u8]) -> Result<Self, ControlError> {
        if !matches!(raw.len(), BASIC_SIZE | EXTENDED_SIZE) {
            return Err(ControlError::Length(raw.len()));
        }
        require(raw[23] <= 1, "quick_sharp_enabled")?;
        require(raw[35] <= 1, "custom_curve_enabled")?;
        let name_bytes = &raw[2..22];
        let nul = name_bytes
            .iter()
            .position(|b| *b == 0)
            .ok_or(ControlError::Field("registration_name terminator"))?;
        require(
            name_bytes[nul..].iter().all(|b| *b == 0),
            "registration_name padding",
        )?;
        require(
            name_bytes[..nul].iter().all(|b| (32..=126).contains(b)),
            "registration_name ASCII",
        )?;
        // ASCII validation above makes the conversion infallible.
        let registration_name = String::from_utf8(name_bytes[..nul].to_vec())
            .map_err(|_| ControlError::Field("registration_name"))?;
        let curve = CurveData {
            enabled: raw[35] == 1,
            bytes: if raw.len() == EXTENDED_SIZE {
                Some(Box::new(
                    raw[BASIC_SIZE..]
                        .try_into()
                        .map_err(|_| ControlError::Length(raw.len()))?,
                ))
            } else {
                None
            },
        };
        let value = Self {
            base: u16::from_le_bytes([raw[0], raw[1]]),
            registration_name,
            apply_level: raw[22],
            quick_sharp_enabled: raw[23] == 1,
            quick_sharp: raw[24] as i8,
            sharpening_quarters: raw[25] as i8,
            middle_range_sharpening_quarters: raw[26] as i8,
            clarity_quarters: raw[27] as i8,
            contrast_quarters: raw[28] as i8,
            brightness_quarters: raw[29] as i8,
            saturation_quarters: raw[30] as i8,
            hue_quarters: raw[31] as i8,
            filter_effects: raw[32],
            toning: raw[33],
            toning_density_quarters: raw[34] as i8,
            curve,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn encode(&self) -> Result<Vec<u8>, ControlError> {
        self.validate()?;
        let mut raw = vec![0; BASIC_SIZE];
        raw[..2].copy_from_slice(&self.base.to_le_bytes());
        raw[2..2 + self.registration_name.len()].copy_from_slice(self.registration_name.as_bytes());
        raw[22..].copy_from_slice(&[
            self.apply_level,
            u8::from(self.quick_sharp_enabled),
            self.quick_sharp as u8,
            self.sharpening_quarters as u8,
            self.middle_range_sharpening_quarters as u8,
            self.clarity_quarters as u8,
            self.contrast_quarters as u8,
            self.brightness_quarters as u8,
            self.saturation_quarters as u8,
            self.hue_quarters as u8,
            self.filter_effects,
            self.toning,
            self.toning_density_quarters as u8,
            u8::from(self.curve.enabled),
        ]);
        if let Some(bytes) = &self.curve.bytes {
            raw.extend_from_slice(bytes.as_ref());
        }
        Ok(raw)
    }

    /// Validation is separate from transport success. A caller must also query
    /// current camera capabilities, back up the destination and verify readback.
    pub fn validate_for_write(&self, destination: u16) -> Result<(), ControlError> {
        if !(201..=209).contains(&destination) {
            return Err(ControlError::Destination(destination));
        }
        if self.base == 0 {
            return Err(ControlError::UnusedSlot);
        }
        self.validate()
    }

    pub fn validate(&self) -> Result<(), ControlError> {
        if !matches!(self.base, 0..=8 | 101..=120) {
            return Err(ControlError::UnsupportedBase(self.base));
        }
        require(
            self.registration_name.len() <= 19
                && self
                    .registration_name
                    .bytes()
                    .all(|b| (32..=126).contains(&b)),
            "registration_name",
        )?;
        require(
            self.base == 0 || !self.registration_name.trim().is_empty(),
            "registration_name",
        )?;
        if self.curve.enabled {
            let bytes = self
                .curve
                .bytes
                .as_ref()
                .ok_or(ControlError::Field("enabled curve missing payload"))?;
            require(
                bytes[0] != 0 || bytes[1] != 0,
                "enabled curve missing host header",
            )?;
            // Header conventions and sample ranges belong to the source. Unknown
            // headers remain opaque; do not invent a LUT or normalize its samples.
        }
        // Fields of an unused slot do not describe an executable Picture Control.
        // Preserve structurally valid readbacks but never permit writing them.
        if self.base == 0 {
            return Ok(());
        }
        let standard = (1..=7).contains(&self.base);
        let monochrome = self.base == 4;
        let auto = self.base == 8;
        let creative = (101..=120).contains(&self.base);
        require(
            if creative {
                self.apply_level <= 100 && self.apply_level % 10 == 0
            } else {
                self.apply_level == 0
            },
            "apply_level",
        )?;
        let auto_sharp = standard && self.quick_sharp_enabled && self.quick_sharp == i8::MIN;
        require(
            (-2..=2).contains(&self.quick_sharp) || auto_sharp,
            "quick_sharp",
        )?;
        let auto_step = |value: i8| matches!(value, -8 | -4 | 0 | 4 | 8);
        for (field, value, low, high) in [
            ("sharpening", self.sharpening_quarters, -12, 36),
            (
                "middle_range_sharpening",
                self.middle_range_sharpening_quarters,
                -20,
                20,
            ),
            ("clarity", self.clarity_quarters, -20, 20),
        ] {
            require(
                if auto {
                    auto_step(value)
                } else {
                    (low..=high).contains(&value) || (value == i8::MIN && auto_sharp)
                },
                field,
            )?;
        }
        require(
            if auto {
                auto_step(self.contrast_quarters)
            } else {
                (-12..=12).contains(&self.contrast_quarters)
                    || (standard && self.contrast_quarters == i8::MIN)
            },
            "contrast",
        )?;
        require(
            if standard {
                (-6..=6).contains(&self.brightness_quarters)
            } else {
                self.brightness_quarters == 0
            },
            "brightness",
        )?;
        require(
            if auto {
                auto_step(self.saturation_quarters)
            } else if monochrome || self.base >= 113 {
                self.saturation_quarters == 0
            } else {
                (-12..=12).contains(&self.saturation_quarters)
                    || (standard && self.saturation_quarters == i8::MIN)
            },
            "saturation",
        )?;
        require(
            if (standard && !monochrome) || (101..=104).contains(&self.base) {
                (-12..=12).contains(&self.hue_quarters)
            } else {
                self.hue_quarters == 0
            },
            "hue",
        )?;
        require(
            if monochrome || self.base >= 113 {
                self.filter_effects <= 4
            } else {
                self.filter_effects == 0
            },
            "filter_effects",
        )?;
        require(
            if monochrome || self.base >= 117 {
                self.toning <= 9
            } else {
                self.toning == 0
            },
            "toning",
        )?;
        require(
            if monochrome || self.base >= 113 {
                (0..=28).contains(&self.toning_density_quarters)
            } else {
                self.toning_density_quarters == 0
            },
            "toning_density",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A deliberately constructed protocol fixture, not a downloaded NP3.
    fn standard_bytes() -> Vec<u8> {
        let mut raw = vec![0; 36];
        raw[0] = 1;
        raw[2..9].copy_from_slice(b"Test PC");
        raw[25] = 3; // +0.75 sharpening, represented in quarter steps.
        raw[26] = 1;
        raw[27] = (-2_i8) as u8;
        raw[28] = (-12_i8) as u8;
        raw[29] = 6;
        raw[30] = 12;
        raw[31] = (-1_i8) as u8;
        raw
    }

    #[test]
    fn signed_native_quarters_and_offsets_roundtrip_without_scaling() {
        let raw = standard_bytes();
        let pc = PictureControl::decode(&raw).unwrap();
        assert_eq!(pc.registration_name, "Test PC");
        assert_eq!(pc.sharpening_quarters, 3);
        assert_eq!(pc.clarity_quarters, -2);
        assert_eq!(pc.hue_quarters, -1);
        assert_eq!(pc.encode().unwrap(), raw);
        pc.validate_for_write(201).unwrap();
    }

    #[test]
    fn length_and_flag_must_describe_a_complete_dataset() {
        let raw = standard_bytes();
        for size in [0, 2, 35, 37, 613, 615] {
            let mut broken = raw.clone();
            broken.resize(size, 0);
            assert!(PictureControl::decode(&broken).is_err());
        }
        for offset in [23, 35] {
            let mut broken = raw.clone();
            broken[offset] = 2;
            assert!(PictureControl::decode(&broken).is_err());
        }
        let mut broken = raw;
        broken[35] = 1;
        assert!(PictureControl::decode(&broken).is_err());
    }

    #[test]
    fn enabled_curve_preserves_all_257_pairs_and_opaque_header() {
        let mut raw = standard_bytes();
        raw[35] = 1;
        raw.resize(614, 0);
        raw[36..38].copy_from_slice(b"I0");
        // Preserve arbitrary samples: the documented transport does not require monotonic LUTs.
        for i in 0..514 {
            raw[100 + i] = ((i * 17) % 256) as u8;
        }
        let pc = PictureControl::decode(&raw).unwrap();
        pc.validate_for_write(209).unwrap();
        assert_eq!(pc.encode().unwrap(), raw);
        let mut missing_header = raw;
        missing_header[36..100].fill(0);
        assert!(PictureControl::decode(&missing_header).is_err());
    }

    #[test]
    fn disabled_curve_tail_is_preserved_without_treating_it_as_active() {
        let mut raw = standard_bytes();
        raw.resize(614, 0xAB);
        let pc = PictureControl::decode(&raw).unwrap();
        assert!(!pc.curve.enabled);
        assert_eq!(pc.encode().unwrap(), raw);
    }

    #[test]
    fn unused_slots_can_be_backed_up_but_not_uploaded() {
        for size in [36, 614] {
            let raw = vec![0; size];
            let pc = PictureControl::decode(&raw).unwrap();
            assert_eq!(pc.encode().unwrap(), raw);
            assert!(matches!(
                pc.validate_for_write(201),
                Err(ControlError::UnusedSlot)
            ));
        }
    }

    #[test]
    fn rejects_unknown_bases_and_destinations_including_cloud_slots() {
        for base in [9_u16, 11, 12, 99, 121, 201, 301, u16::MAX] {
            let mut raw = standard_bytes();
            raw[..2].copy_from_slice(&base.to_le_bytes());
            assert!(PictureControl::decode(&raw).is_err());
        }
        let pc = PictureControl::decode(&standard_bytes()).unwrap();
        for slot in [0, 1, 200, 210, 301] {
            assert!(pc.validate_for_write(slot).is_err());
        }
    }

    #[test]
    fn names_are_ascii_terminated_and_never_silently_truncated() {
        let raw = standard_bytes();
        for name in [
            "",
            " ",
            "你好",
            "12345678901234567890",
            "line\nname",
            "a\0b",
        ] {
            let mut pc = PictureControl::decode(&raw).unwrap();
            pc.registration_name = name.into();
            assert!(pc.encode().is_err());
        }
        let mut pc = PictureControl::decode(&raw).unwrap();
        pc.registration_name = "1234567890123456789".into();
        let encoded = pc.encode().unwrap();
        assert_eq!(encoded[21], 0);
        assert_eq!(PictureControl::decode(&encoded).unwrap(), pc);
        let mut missing_nul = raw.clone();
        missing_nul[2..22].fill(b'x');
        assert!(PictureControl::decode(&missing_nul).is_err());
        let mut hidden_suffix = raw;
        hidden_suffix[15] = b'x';
        assert!(PictureControl::decode(&hidden_suffix).is_err());
    }

    #[test]
    fn validates_individual_native_ranges_and_does_not_clamp() {
        let raw = standard_bytes();
        for (offset, invalid) in [
            (22, 10),
            (24, 3),
            (25, 37),
            (26, 21),
            (27, 21),
            (28, 13),
            (29, 7),
            (30, 13),
            (31, 13),
            (32, 1),
            (33, 1),
            (34, 1),
        ] {
            let mut bad = raw.clone();
            bad[offset] = invalid;
            assert!(PictureControl::decode(&bad).is_err(), "offset {offset}");
        }
    }

    #[test]
    fn automatic_sharpening_requires_compatible_base_and_quick_sharp_flag() {
        let mut raw = standard_bytes();
        raw[24] = 128;
        assert!(PictureControl::decode(&raw).is_err());
        raw[23] = 1;
        for offset in 25..=27 {
            raw[offset] = 128;
        }
        assert!(PictureControl::decode(&raw).is_ok());
        raw[24] = 0;
        assert!(PictureControl::decode(&raw).is_err());
    }

    #[test]
    fn base_specific_controls_are_validated() {
        let mut raw = standard_bytes();
        raw[0] = 4;
        raw[30] = 0;
        raw[31] = 0;
        raw[32] = 4;
        raw[33] = 9;
        raw[34] = 28;
        assert!(PictureControl::decode(&raw).is_ok());
        raw[30] = 1;
        assert!(PictureControl::decode(&raw).is_err());
        let mut creative = standard_bytes();
        creative[0] = 113;
        creative[22] = 100;
        creative[29] = 0;
        creative[30] = 0;
        creative[31] = 0;
        creative[32] = 4;
        creative[34] = 28;
        assert!(PictureControl::decode(&creative).is_ok());
        creative[33] = 1; // Fixed-color creative controls do not allow selecting a toning color.
        assert!(PictureControl::decode(&creative).is_err());
    }

    #[test]
    fn auto_base_uses_discrete_tuning_steps() {
        let mut raw = vec![0; 36];
        raw[0] = 8;
        raw[2] = b'A';
        for offset in [25, 26, 27, 28, 30] {
            raw[offset] = (-8_i8) as u8;
        }
        assert!(PictureControl::decode(&raw).is_ok());
        raw[25] = 1;
        assert!(PictureControl::decode(&raw).is_err());
    }
}
