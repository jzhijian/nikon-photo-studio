//! Bounded inspection of NP3 TLV containers. This preserves the original file;
//! it does not convert Nikon adjustments into our generic photo editor values.
//! Layout evidence: two local 0310 samples, cross-checked against the author's
//! parser at PkmX/nkfl-rs commit 933683e4c3f8a7e42e58e704345c68bbceae3c16.
//! Parsing below is application-owned and deliberately rejects ambiguous input.

use serde::Serialize;
use std::{collections::BTreeSet, io::Read, ops::Range, path::Path};
use thiserror::Error;

/// Application resource limits, not Nikon's documented format limits.
pub const MAX_FILE_SIZE: usize = 1024 * 1024;
pub const MAX_TAG_COUNT: usize = 1024;
const FLEXIBLE_TAGS: [u32; 8] = [
    0x1900, 0x1A00, 0x1B00, 0x1C00, 0x1D00, 0x1E00, 0x1F00, 0x2000,
];

#[derive(Debug, Error)]
pub enum InspectionError {
    #[error("NP3 file exceeds the application size limit: {0} bytes")]
    TooLarge(usize),
    #[error("Not a supported Nikon NCP TLV container")]
    Magic,
    #[error("Truncated Nikon container at byte {0}")]
    Truncated(usize),
    #[error("Repeated Nikon field 0x{0:08X}; refusing ambiguous values")]
    DuplicateTag(u32),
    #[error("Nikon container has {0} bytes after its terminator")]
    TrailingData(usize),
    #[error("Nikon container exceeds the application tag-count limit")]
    TooManyTags,
    #[error("Nikon container is missing the NP3 version field")]
    MissingVersion,
    #[error("Unsupported NP3 version-field encoding")]
    VersionEncoding,
    #[error(
        "Unsupported 0310 layout for tag 0x{tag:08X}: expected {expected} bytes, got {actual}"
    )]
    UnsupportedLayout {
        tag: u32,
        expected: usize,
        actual: usize,
    },
    #[error("Could not read Nikon preset: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RenderCoverageError {
    #[error("Renderer has no declared support for NP3 version {0}")]
    UnsupportedVersion(String),
    #[error("Renderer would omit unhandled Nikon fields: {0:?}")]
    UnsupportedTags(Vec<u32>),
}

#[derive(Debug, Clone)]
struct Entry {
    tag: u32,
    payload: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct NikonFile {
    original: Vec<u8>,
    entries: Vec<Entry>,
    version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagInspection {
    pub tag: u32,
    pub tag_hex: String,
    pub payload_offset: usize,
    pub payload_length: usize,
    pub known_layout: bool,
    /// A structural label only, not a claim that adjustment semantics are decoded.
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NikonInspection {
    pub schema_version: u32,
    pub format: String,
    pub file_size: usize,
    pub version: String,
    pub recognized_version: bool,
    pub display_name: Option<String>,
    pub tags: Vec<TagInspection>,
    pub unknown_tags: Vec<u32>,
    pub has_custom_curve: bool,
    pub has_all_flexible_color_blocks: bool,
    pub flexible_color_blocks_present: Vec<u32>,
    pub rendering_verified: bool,
    pub source_verified: bool,
    pub limitations: Vec<String>,
}

fn expected_size(tag: u32) -> Option<usize> {
    match tag {
        0x0002 => Some(578),
        0x0100 => Some(4),
        0x0200 => Some(20),
        0x0300..=0x1E00 if tag % 0x100 == 0 => Some(2),
        0x1F00 => Some(28),
        0x2000 => Some(20),
        _ => None,
    }
}

fn label(tag: u32) -> &'static str {
    match tag {
        0x0002 => "custom-curve-block",
        0x0100 => "format-version",
        0x0200 => "display-name",
        0x0300 => "base-profile-block",
        0x0400..=0x1800 if tag % 0x100 == 0 => "opaque-adjustment-block",
        0x1900..=0x1E00 if tag % 0x100 == 0 => "flexible-color-adjustment-block",
        0x1F00 => "flexible-color-blender-block",
        0x2000 => "flexible-color-grading-block",
        _ => "unknown-preserved-block",
    }
}

fn word(raw: &[u8], cursor: &mut usize) -> Result<u32, InspectionError> {
    let next = cursor
        .checked_add(4)
        .ok_or(InspectionError::Truncated(*cursor))?;
    let bytes: [u8; 4] = raw
        .get(*cursor..next)
        .ok_or(InspectionError::Truncated(*cursor))?
        .try_into()
        .map_err(|_| InspectionError::Truncated(*cursor))?;
    *cursor = next;
    Ok(u32::from_be_bytes(bytes))
}

impl NikonFile {
    pub fn parse(raw: &[u8]) -> Result<Self, InspectionError> {
        if raw.len() > MAX_FILE_SIZE {
            return Err(InspectionError::TooLarge(raw.len()));
        }
        if !raw.starts_with(b"NCP\0") {
            return Err(InspectionError::Magic);
        }
        let mut cursor = 4;
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        loop {
            let tag = word(raw, &mut cursor)?;
            if tag == 0 {
                if cursor != raw.len() {
                    return Err(InspectionError::TrailingData(raw.len() - cursor));
                }
                break;
            }
            if !seen.insert(tag) {
                return Err(InspectionError::DuplicateTag(tag));
            }
            if entries.len() == MAX_TAG_COUNT {
                return Err(InspectionError::TooManyTags);
            }
            let length = word(raw, &mut cursor)? as usize;
            let end = cursor
                .checked_add(length)
                .filter(|end| *end <= raw.len())
                .ok_or(InspectionError::Truncated(cursor))?;
            entries.push(Entry {
                tag,
                payload: cursor..end,
            });
            cursor = end;
        }
        let version_range = entries
            .iter()
            .find(|entry| entry.tag == 0x0100)
            .ok_or(InspectionError::MissingVersion)?
            .payload
            .clone();
        let bytes = &raw[version_range];
        if bytes.len() != 4 || !bytes.iter().all(u8::is_ascii_digit) {
            return Err(InspectionError::VersionEncoding);
        }
        let version = std::str::from_utf8(bytes)
            .map_err(|_| InspectionError::VersionEncoding)?
            .to_owned();
        if version == "0310" {
            for entry in &entries {
                if let Some(expected) = expected_size(entry.tag) {
                    let actual = entry.payload.len();
                    if actual != expected {
                        return Err(InspectionError::UnsupportedLayout {
                            tag: entry.tag,
                            expected,
                            actual,
                        });
                    }
                }
            }
        }
        Ok(Self {
            original: raw.to_vec(),
            entries,
            version,
        })
    }

    pub fn read(path: &Path) -> Result<Self, InspectionError> {
        let mut raw = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_FILE_SIZE as u64 + 1)
            .read_to_end(&mut raw)?;
        Self::parse(&raw)
    }

    pub fn original_bytes(&self) -> &[u8] {
        &self.original
    }

    pub fn tag_bytes(&self, tag: u32) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|entry| entry.tag == tag)
            .map(|entry| &self.original[entry.payload.clone()])
    }

    /// This checks declared field coverage, not visual equivalence. A renderer
    /// must still prove that it preserves each field's meaning through its engine.
    pub fn require_renderer_coverage(
        &self,
        versions: &[&str],
        handled_tags: &[u32],
    ) -> Result<(), RenderCoverageError> {
        if !versions.contains(&self.version.as_str()) {
            return Err(RenderCoverageError::UnsupportedVersion(
                self.version.clone(),
            ));
        }
        let omitted: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| (!handled_tags.contains(&entry.tag)).then_some(entry.tag))
            .collect();
        if !omitted.is_empty() {
            return Err(RenderCoverageError::UnsupportedTags(omitted));
        }
        Ok(())
    }

    pub fn inspection(&self) -> NikonInspection {
        let recognized = self.version == "0310";
        let known = |tag| tag == 0x0100 || (recognized && expected_size(tag).is_some());
        let tags = self
            .entries
            .iter()
            .map(|entry| TagInspection {
                tag: entry.tag,
                tag_hex: format!("0x{:08X}", entry.tag),
                payload_offset: entry.payload.start,
                payload_length: entry.payload.len(),
                known_layout: known(entry.tag),
                label: if known(entry.tag) {
                    label(entry.tag)
                } else {
                    "unknown-preserved-block"
                }
                .into(),
            })
            .collect();
        let unknown_tags = self
            .entries
            .iter()
            .filter_map(|entry| (!known(entry.tag)).then_some(entry.tag))
            .collect();
        let flexible_color_blocks_present: Vec<_> = FLEXIBLE_TAGS
            .into_iter()
            .filter(|tag| recognized && self.tag_bytes(*tag).is_some())
            .collect();
        let display_name = self.tag_bytes(0x0200).and_then(|raw| {
            if !recognized {
                return None;
            }
            let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
            let name = std::str::from_utf8(&raw[..end]).ok()?;
            if name.trim().is_empty() || name.chars().any(char::is_control) {
                None
            } else {
                Some(name.to_owned())
            }
        });
        NikonInspection {
            schema_version: 1, format: "nikon-ncp-tlv".into(), file_size: self.original.len(),
            version: self.version.clone(), recognized_version: recognized, display_name,
            tags, unknown_tags, has_custom_curve: recognized && self.tag_bytes(0x0002).is_some(),
            has_all_flexible_color_blocks: flexible_color_blocks_present.len() == FLEXIBLE_TAGS.len(),
            flexible_color_blocks_present, rendering_verified: false, source_verified: false,
            limitations: vec![
                "Only container structure is checked; Nikon adjustment values remain opaque.".into(),
                "No source authentication or complete-rendering equivalence has been verified.".into(),
                "A rendering backend must explicitly handle every original field, including curves and unknown blocks.".into(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(file: &mut Vec<u8>, tag: u32, value: &[u8]) {
        file.extend_from_slice(&tag.to_be_bytes());
        file.extend_from_slice(&(value.len() as u32).to_be_bytes());
        file.extend_from_slice(value);
    }
    fn fixture(extra: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut result = b"NCP\0".to_vec();
        field(&mut result, 0x0100, b"0310");
        let mut name = vec![0; 20];
        name[..8].copy_from_slice(b"Own Test");
        field(&mut result, 0x0200, &name);
        for (tag, value) in extra {
            field(&mut result, *tag, value);
        }
        result.extend_from_slice(&0_u32.to_be_bytes());
        result
    }

    #[test]
    fn preserves_unknown_blocks_and_exact_original_bytes() {
        let raw = fixture(&[(0xDEAD, vec![0x00, 0x80, 0xFF]), (0xBEEF, vec![])]);
        let file = NikonFile::parse(&raw).unwrap();
        assert_eq!(file.original_bytes(), raw);
        assert_eq!(file.tag_bytes(0xDEAD), Some(&[0x00, 0x80, 0xFF][..]));
        let report = file.inspection();
        assert_eq!(report.version, "0310");
        assert_eq!(report.display_name.as_deref(), Some("Own Test"));
        assert_eq!(report.unknown_tags, vec![0xDEAD, 0xBEEF]);
        assert!(!report.rendering_verified);
        assert!(!report.source_verified);
        assert_eq!(serde_json::to_value(report).unwrap()["fileSize"], raw.len());
    }

    #[test]
    fn rejects_every_truncated_prefix_and_wrong_magic() {
        let raw = fixture(&[(0x0300, vec![0x00, 0x20])]);
        for size in 0..raw.len() {
            assert!(NikonFile::parse(&raw[..size]).is_err(), "prefix {size}");
        }
        let mut wrong = raw;
        wrong[..4].copy_from_slice(b"NP3X");
        assert!(NikonFile::parse(&wrong).is_err());
    }

    #[test]
    fn rejects_duplicate_fields_and_bytes_after_terminator() {
        let duplicate = fixture(&[(0x0100, b"0310".to_vec())]);
        assert!(matches!(
            NikonFile::parse(&duplicate),
            Err(InspectionError::DuplicateTag(0x0100))
        ));
        let duplicate_unknown = fixture(&[(0xAAAA, vec![]), (0xAAAA, vec![1])]);
        assert!(matches!(
            NikonFile::parse(&duplicate_unknown),
            Err(InspectionError::DuplicateTag(0xAAAA))
        ));
        for suffix in [&[0_u8][..], &[0; 4][..], b"extra"] {
            let mut raw = fixture(&[]);
            raw.extend_from_slice(suffix);
            assert!(matches!(
                NikonFile::parse(&raw),
                Err(InspectionError::TrailingData(_))
            ));
        }
    }

    #[test]
    fn enforces_size_and_field_count_limits_before_allocating_payloads() {
        let huge = vec![0; MAX_FILE_SIZE + 1];
        assert!(matches!(
            NikonFile::parse(&huge),
            Err(InspectionError::TooLarge(_))
        ));
        let mut declared_huge = fixture(&[]);
        declared_huge.truncate(4);
        declared_huge.extend_from_slice(&0x0100_u32.to_be_bytes());
        declared_huge.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(NikonFile::parse(&declared_huge).is_err());
        let extras: Vec<_> = (0..MAX_TAG_COUNT)
            .map(|i| (0x10000 + i as u32, vec![]))
            .collect();
        assert!(matches!(
            NikonFile::parse(&fixture(&extras)),
            Err(InspectionError::TooManyTags)
        ));
    }

    #[test]
    fn requires_explicit_version_and_reports_unknown_versions_without_guessing() {
        assert!(matches!(
            NikonFile::parse(b"NCP\0\0\0\0\0"),
            Err(InspectionError::MissingVersion)
        ));
        let mut raw = fixture(&[]);
        raw[12..16].copy_from_slice(b"9999");
        let file = NikonFile::parse(&raw).unwrap();
        assert!(!file.inspection().recognized_version);
        assert!(file
            .require_renderer_coverage(&["0310"], &[0x0100, 0x0200])
            .is_err());
        let mut malformed = fixture(&[]);
        malformed[12] = 0xFF;
        assert!(NikonFile::parse(&malformed).is_err());
    }

    #[test]
    fn distinguishes_flexible_blocks_from_a_complete_renderer() {
        let mut extras: Vec<_> = (0x1900..=0x1E00)
            .step_by(0x100)
            .map(|tag| (tag, vec![0x80, 1]))
            .collect();
        extras.push((0x1F00, vec![0x80; 28]));
        extras.push((0x2000, vec![0x80; 20]));
        let file = NikonFile::parse(&fixture(&extras)).unwrap();
        assert!(file.inspection().has_all_flexible_color_blocks);
        assert!(!file.inspection().rendering_verified);
        assert!(file
            .require_renderer_coverage(&["0310"], &[0x0100, 0x0200])
            .is_err());
        extras.pop();
        assert!(
            !NikonFile::parse(&fixture(&extras))
                .unwrap()
                .inspection()
                .has_all_flexible_color_blocks
        );
    }

    #[test]
    fn curve_is_never_silently_ignored_by_renderer_coverage_checks() {
        let raw = fixture(&[(0x0002, vec![0; 578])]);
        let file = NikonFile::parse(&raw).unwrap();
        assert!(file.inspection().has_custom_curve);
        assert_eq!(file.tag_bytes(0x0002).unwrap().len(), 578);
        assert!(
            matches!(file.require_renderer_coverage(&["0310"], &[0x0100, 0x0200]),
            Err(RenderCoverageError::UnsupportedTags(tags)) if tags == vec![0x0002])
        );
    }

    #[test]
    fn known_version_layout_mismatches_fail_instead_of_partial_parsing() {
        for (tag, size) in [
            (0x0002, 577),
            (0x0300, 1),
            (0x1900, 1),
            (0x1F00, 24),
            (0x2000, 19),
        ] {
            assert!(
                matches!(
                    NikonFile::parse(&fixture(&[(tag, vec![0; size])])),
                    Err(InspectionError::UnsupportedLayout { .. })
                ),
                "tag {tag:x}"
            );
        }
    }

    #[test]
    #[ignore = "Requires local third-party NP3 samples; never checked into the source tree"]
    fn real_np3_samples_remain_unchanged_and_curves_are_detected() {
        let directory =
            std::env::var("NPS_TEST_NP3_DIRECTORY").expect("Set NPS_TEST_NP3_DIRECTORY explicitly");
        for (name, size, curve) in [
            ("Vibrant_Lamoureux.NP3", 392, false),
            ("VintageTouch_Amy.NP3", 978, true),
        ] {
            let path = std::path::Path::new(&directory).join(name);
            let original = std::fs::read(&path).unwrap();
            let file = NikonFile::read(&path).unwrap();
            let report = file.inspection();
            assert_eq!(report.file_size, size);
            assert_eq!(report.version, "0310");
            assert_eq!(report.has_custom_curve, curve);
            assert!(report.has_all_flexible_color_blocks);
            assert!(report.unknown_tags.is_empty());
            assert_eq!(file.original_bytes(), original);
            assert_eq!(std::fs::read(path).unwrap(), original);
        }
    }
}
