//! SQLite database for photos, presets, and edit history

use rusqlite::{Connection, Result as SqlResult};
use std::path::Path;
use std::sync::Mutex;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn: Mutex::new(conn) };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS photos (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL UNIQUE,
                filename TEXT NOT NULL,
                format TEXT NOT NULL,
                width INTEGER DEFAULT 0,
                height INTEGER DEFAULT 0,
                file_size INTEGER DEFAULT 0,
                date_taken TEXT,
                camera_model TEXT,
                lens TEXT,
                aperture REAL,
                shutter_speed TEXT,
                iso INTEGER,
                focal_length REAL,
                blur_score REAL,
                category TEXT,
                is_deleted INTEGER DEFAULT 0,
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS presets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                format TEXT NOT NULL,
                params_json TEXT NOT NULL,
                lut_path TEXT,
                source TEXT DEFAULT 'imported',
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS edits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                photo_id INTEGER NOT NULL,
                params_json TEXT NOT NULL,
                preset_id INTEGER,
                applied_at TEXT DEFAULT (datetime('now')),
                FOREIGN KEY (photo_id) REFERENCES photos(id),
                FOREIGN KEY (preset_id) REFERENCES presets(id)
            );

            CREATE INDEX IF NOT EXISTS idx_photos_category ON photos(category);
            CREATE INDEX IF NOT EXISTS idx_photos_blur ON photos(blur_score);
            CREATE INDEX IF NOT EXISTS idx_photos_deleted ON photos(is_deleted);
            CREATE INDEX IF NOT EXISTS idx_edits_photo ON edits(photo_id);
            ",
        )?;
        Ok(())
    }

    /// Insert a photo record. Returns the new row id, or 0 if ignored (duplicate).
    pub fn insert_photo(&self, file_path: &str, filename: &str, format: &str) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO photos (file_path, filename, format) VALUES (?1, ?2, ?3)",
            rusqlite::params![file_path, filename, format],
        )?;
        if conn.changes() == 0 {
            Ok(0) // duplicate ignored
        } else {
            Ok(conn.last_insert_rowid())
        }
    }

    /// Update blur score for a photo
    pub fn update_blur_score(&self, photo_id: i64, score: f32) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE photos SET blur_score = ?1 WHERE id = ?2",
            rusqlite::params![score, photo_id],
        )?;
        Ok(())
    }

    /// Update category for a photo
    pub fn update_category(&self, photo_id: i64, category: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE photos SET category = ?1 WHERE id = ?2",
            rusqlite::params![category, photo_id],
        )?;
        Ok(())
    }

    /// Mark photo as deleted (soft delete)
    pub fn soft_delete(&self, photo_id: i64) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE photos SET is_deleted = 1 WHERE id = ?1",
            rusqlite::params![photo_id],
        )?;
        Ok(())
    }

    /// Restore a soft-deleted photo
    pub fn restore(&self, photo_id: i64) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE photos SET is_deleted = 0 WHERE id = ?1",
            rusqlite::params![photo_id],
        )?;
        Ok(())
    }

    /// Get blurry photos below threshold
    pub fn get_blurry_photos(&self, threshold: f32) -> SqlResult<Vec<(i64, String, f32)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, file_path, blur_score FROM photos WHERE blur_score < ?1 AND is_deleted = 0 ORDER BY blur_score ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![threshold], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.collect()
    }

    /// Insert a preset
    pub fn insert_preset(&self, name: &str, format: &str, params_json: &str, source: &str) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO presets (name, format, params_json, source) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![name, format, params_json, source],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Record an edit in history
    pub fn record_edit(&self, photo_id: i64, params_json: &str, preset_id: Option<i64>) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO edits (photo_id, params_json, preset_id) VALUES (?1, ?2, ?3)",
            rusqlite::params![photo_id, params_json, preset_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        Database::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_init_schema() {
        let db = test_db();
        // Should not panic on repeated init
        db.init_schema().unwrap();
    }

    #[test]
    fn test_insert_photo() {
        let db = test_db();
        let id = db.insert_photo("/photos/DSC_001.NEF", "DSC_001.NEF", "NEF").unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_insert_duplicate_photo() {
        let db = test_db();
        db.insert_photo("/photos/DSC_001.NEF", "DSC_001.NEF", "NEF").unwrap();
        // Should not fail on duplicate (INSERT OR IGNORE)
        let id2 = db.insert_photo("/photos/DSC_001.NEF", "DSC_001.NEF", "NEF").unwrap();
        assert_eq!(id2, 0); // ignored
    }

    #[test]
    fn test_update_blur_score() {
        let db = test_db();
        let id = db.insert_photo("/photos/test.jpg", "test.jpg", "JPG").unwrap();
        db.update_blur_score(id, 42.5).unwrap();
    }

    #[test]
    fn test_update_category() {
        let db = test_db();
        let id = db.insert_photo("/photos/test.jpg", "test.jpg", "JPG").unwrap();
        db.update_category(id, "portrait").unwrap();
    }

    #[test]
    fn test_soft_delete_and_restore() {
        let db = test_db();
        let id = db.insert_photo("/photos/test.jpg", "test.jpg", "JPG").unwrap();
        db.soft_delete(id).unwrap();
        db.restore(id).unwrap();
    }

    #[test]
    fn test_get_blurry_photos() {
        let db = test_db();
        let id1 = db.insert_photo("/photos/sharp.jpg", "sharp.jpg", "JPG").unwrap();
        let id2 = db.insert_photo("/photos/blurry.jpg", "blurry.jpg", "JPG").unwrap();
        db.update_blur_score(id1, 80.0).unwrap();
        db.update_blur_score(id2, 20.0).unwrap();

        let blurry = db.get_blurry_photos(60.0).unwrap();
        assert_eq!(blurry.len(), 1);
        assert_eq!(blurry[0].1, "/photos/blurry.jpg");
    }

    #[test]
    fn test_insert_preset() {
        let db = test_db();
        let id = db.insert_preset("Warm Glow", "xmp", r#"{"exposure":0.5}"#, "imported").unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_record_edit() {
        let db = test_db();
        let photo_id = db.insert_photo("/photos/test.jpg", "test.jpg", "JPG").unwrap();
        let preset_id = db.insert_preset("Test", "xmp", "{}", "imported").unwrap();
        db.record_edit(photo_id, r#"{"exposure":1.0}"#, Some(preset_id)).unwrap();
    }
}
