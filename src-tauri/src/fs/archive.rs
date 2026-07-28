//! Archive/organize files into categorized folders

use std::path::Path;

/// Move a file to an archive folder, creating it if needed
pub fn archive_file(src: &str, dest_dir: &str) -> Result<String, String> {
    std::fs::create_dir_all(dest_dir).map_err(|e| format!("Cannot create dir: {}", e))?;

    let filename = Path::new(src)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid source filename")?;

    let dest = Path::new(dest_dir).join(filename);

    // Handle name conflicts
    let dest = if dest.exists() {
        let stem = Path::new(filename).file_stem().and_then(|s| s.to_str()).unwrap_or("file");
        let ext = Path::new(filename).extension().and_then(|e| e.to_str()).unwrap_or("");
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Path::new(dest_dir).join(format!("{}_{}.{}", stem, ts, ext))
    } else {
        dest
    };

    std::fs::copy(src, &dest).map_err(|e| format!("Copy failed: {}", e))?;
    Ok(dest.to_string_lossy().to_string())
}

/// Organize photos into category folders
pub fn organize_by_category(
    files: &[(String, String)], // (path, category)
    base_dir: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut results = Vec::new();
    for (path, category) in files {
        let dest_dir = Path::new(base_dir).join(category);
        let new_path = archive_file(path, &dest_dir.to_string_lossy())?;
        results.push((path.clone(), new_path));
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_archive_file() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();

        let src = src_dir.path().join("photo.jpg");
        fs::write(&src, b"fake image data").unwrap();

        let dest = dest_dir.path().join("archive");
        let result = archive_file(
            src.to_str().unwrap(),
            dest.to_str().unwrap(),
        );
        assert!(result.is_ok());
        let new_path = result.unwrap();
        assert!(Path::new(&new_path).exists());
        // Original still exists (copy, not move)
        assert!(src.exists());
    }

    #[test]
    fn test_organize_by_category() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();

        let f1 = src_dir.path().join("portrait1.jpg");
        let f2 = src_dir.path().join("landscape1.jpg");
        fs::write(&f1, b"x").unwrap();
        fs::write(&f2, b"x").unwrap();

        let files = vec![
            (f1.to_string_lossy().to_string(), "People".to_string()),
            (f2.to_string_lossy().to_string(), "Landscape".to_string()),
        ];

        let results = organize_by_category(&files, dest_dir.path().to_str().unwrap());
        assert!(results.is_ok());
        let results = results.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].1.contains("People"));
        assert!(results[1].1.contains("Landscape"));
    }
}
