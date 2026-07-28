//! Application-level recycle bin

use std::path::Path;

/// Get the recycle bin directory
pub fn trash_dir() -> Result<String, String> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "Cannot determine home directory")?;
    let trash = Path::new(&home).join(".nikon-photo-studio").join("trash");
    std::fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
    Ok(trash.to_string_lossy().to_string())
}

/// Move file to trash, returns the trash path
pub fn move_to_trash(src: &str) -> Result<String, String> {
    let trash = trash_dir()?;
    let filename = Path::new(src)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid filename")?;

    let dest = Path::new(&trash).join(filename);
    let dest = if dest.exists() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let stem = Path::new(filename).file_stem().and_then(|s| s.to_str()).unwrap_or("f");
        let ext = Path::new(filename).extension().and_then(|e| e.to_str()).unwrap_or("");
        Path::new(&trash).join(format!("{}_{}.{}", stem, ts, ext))
    } else {
        dest
    };

    std::fs::rename(src, &dest).map_err(|e| format!("Move failed: {}", e))?;
    Ok(dest.to_string_lossy().to_string())
}

/// Restore file from trash
pub fn restore(trash_path: &str, dest_path: &str) -> Result<(), String> {
    std::fs::rename(trash_path, dest_path).map_err(|e| format!("Restore failed: {}", e))
}

/// List files in trash
pub fn list_trash() -> Result<Vec<String>, String> {
    let trash = trash_dir()?;
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&trash) {
        for entry in entries.flatten() {
            files.push(entry.path().to_string_lossy().to_string());
        }
    }
    Ok(files)
}

/// Permanently delete a file from trash
pub fn permanent_delete(trash_path: &str) -> Result<(), String> {
    std::fs::remove_file(trash_path).map_err(|e| format!("Delete failed: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_move_and_restore() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("test_photo.jpg");
        fs::write(&src, b"test data").unwrap();

        let trash_path = move_to_trash(src.to_str().unwrap()).unwrap();
        assert!(!src.exists());
        assert!(Path::new(&trash_path).exists());

        let restore_dest = dir.path().join("restored.jpg");
        restore(&trash_path, restore_dest.to_str().unwrap()).unwrap();
        assert!(restore_dest.exists());
        assert!(!Path::new(&trash_path).exists());
    }
}
