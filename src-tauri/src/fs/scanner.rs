//! Directory scanner for image files

use super::FileInfo;
use std::path::Path;

/// Recursively scan a directory for image files
pub fn scan_recursive(root: &str) -> Vec<FileInfo> {
    let mut files = Vec::new();
    scan_dir(Path::new(root), &mut files);
    files.sort_by(|a, b| a.filename.cmp(&b.filename));
    files
}

fn scan_dir(dir: &Path, files: &mut Vec<FileInfo>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_dir(&path, files);
            } else if super::is_supported_image(&path.to_string_lossy()) {
                if let Ok(metadata) = entry.metadata() {
                    files.push(FileInfo {
                        filename: path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        extension: path.extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_uppercase(),
                        path: path.to_string_lossy().to_string(),
                        size: metadata.len(),
                        modified: None,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_scan_recursive() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(dir.path().join("a.jpg"), b"x").unwrap();
        fs::write(dir.path().join("b.nef"), b"x").unwrap();
        fs::write(sub.join("c.jpg"), b"x").unwrap();
        fs::write(dir.path().join("d.txt"), b"x").unwrap();

        let files = scan_recursive(dir.path().to_str().unwrap());
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn test_scan_empty() {
        let dir = tempfile::tempdir().unwrap();
        let files = scan_recursive(dir.path().to_str().unwrap());
        assert!(files.is_empty());
    }
}
