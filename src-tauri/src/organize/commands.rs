use super::blur;
use super::classify::Classifier;
use super::{BlurResult, ClassifyResult, Sensitivity};

#[tauri::command]
pub async fn detect_blur(path: String, sensitivity: String) -> Result<BlurResult, String> {
    let sens = match sensitivity.as_str() {
        "strict" => Sensitivity::Strict,
        "lenient" => Sensitivity::Lenient,
        _ => Sensitivity::Standard,
    };
    blur::analyze_file(&path, sens)
}

/// Detect blur for many photos in a single call. The heavy lifting runs on a
/// blocking thread pool and is parallelized across CPU cores (work-stealing).
/// `use_gpu` enables the accelerated fast path (smaller analysis buffer).
/// Returns one result per successfully-analyzed photo.
#[tauri::command]
pub async fn batch_detect_blur(
    paths: Vec<String>,
    sensitivity: String,
    use_gpu: bool,
) -> Result<Vec<BlurResult>, String> {
    let sens = match sensitivity.as_str() {
        "strict" => Sensitivity::Strict,
        "lenient" => Sensitivity::Lenient,
        _ => Sensitivity::Standard,
    };
    tokio::task::spawn_blocking(move || blur::analyze_batch_parallel(&paths, sens, use_gpu))
        .await
        .map_err(|e| format!("blur task failed: {e}"))
}

#[tauri::command]
pub async fn classify_photo(path: String, model_dir: String) -> Result<ClassifyResult, String> {
    let classifier = Classifier::new(&model_dir)?;
    classifier.classify_file(&path)
}

#[tauri::command]
pub async fn batch_classify(
    paths: Vec<String>,
    model_dir: String,
) -> Result<Vec<ClassifyResult>, String> {
    // Run on a blocking thread so the parallel worker pool does not stall the
    // async runtime.
    tokio::task::spawn_blocking(move || {
        let classifier = Classifier::new(&model_dir)?;
        Ok(classifier.classify_batch(&paths))
    })
    .await
    .map_err(|e| format!("classify task failed: {e}"))?
}

#[tauri::command]
pub async fn move_to_trash(path: String) -> Result<String, String> {
    let trash_dir = dirs_trash()?;
    std::fs::create_dir_all(&trash_dir).map_err(|e| e.to_string())?;

    let filename = std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid filename")?;

    let dest = std::path::Path::new(&trash_dir).join(filename);
    // Handle name conflicts
    let dest = if dest.exists() {
        let stem = std::path::Path::new(filename).file_stem().and_then(|s| s.to_str()).unwrap_or("file");
        let ext = std::path::Path::new(filename).extension().and_then(|e| e.to_str()).unwrap_or("");
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        std::path::Path::new(&trash_dir).join(format!("{}_{}.{}", stem, ts, ext))
    } else {
        dest
    };

    std::fs::rename(&path, &dest).map_err(|e| format!("Move to trash failed: {}", e))?;
    Ok(dest.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn restore_from_trash(trash_path: String, dest_path: String) -> Result<String, String> {
    std::fs::rename(&trash_path, &dest_path)
        .map_err(|e| format!("Restore failed: {}", e))?;
    Ok(dest_path)
}


#[tauri::command]
pub async fn archive_photos(paths: Vec<String>, dest_dir: String) -> Result<Vec<String>, String> {
    let mut results = Vec::new();
    for path in &paths {
        let new_path = crate::fs::archive::archive_file(path, &dest_dir)?;
        results.push(new_path);
    }
    Ok(results)
}

fn dirs_trash() -> Result<String, String> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME"))
        .map_err(|_| "Cannot determine home directory")?;
    Ok(std::path::Path::new(&home)
        .join(".nikon-photo-studio")
        .join("trash")
        .to_string_lossy()
        .to_string())
}
