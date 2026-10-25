//! Session-scoped cache for complete camera source files.
//!
//! Camera previews can be rendered at several resolutions.  Keeping the
//! downloaded source in this cache prevents every preview-size upgrade from
//! transferring the same RAW file again.  A cache instance is intended to be
//! owned by one [`CameraSession`](super::CameraSession) and cleared whenever
//! that session is replaced or disconnected.

use std::{
    collections::HashMap,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use tempfile::{Builder, TempDir};

/// Default maximum number of complete camera originals retained per session.
pub const DEFAULT_MAX_ENTRIES: usize = 3;

/// Default approximate disk budget for complete camera originals (512 MiB).
pub const DEFAULT_MAX_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug)]
struct CacheEntry {
    safe_name: String,
    expected_size: u64,
    path: PathBuf,
    last_used: u64,
}

/// A pending source-file download.
///
/// Callers write the complete camera object to [`Self::part_path`] and then
/// pass the target to [`CameraSourceCache::commit_write`].  Its fields are
/// private so a target can only refer to a path created inside this cache's
/// temporary directory.
#[derive(Debug)]
pub struct CacheWriteTarget {
    handle: u32,
    safe_name: String,
    expected_size: u64,
    generation: u64,
    root: PathBuf,
    // Keep the old session directory alive while a blocking USB/PTP transfer
    // is still writing. `CameraSourceCache::clear` may run concurrently after
    // a connection generation change; the pending target must remain a valid
    // staging location until that old operation notices it was superseded.
    root_guard: Arc<TempDir>,
    part_path: PathBuf,
    final_path: PathBuf,
}

impl CacheWriteTarget {
    /// Temporary path to which the camera object must be written.
    pub fn part_path(&self) -> &Path {
        &self.part_path
    }

    /// Expected byte length recorded by the camera.
    pub fn expected_size(&self) -> u64 {
        self.expected_size
    }

    /// Camera object handle associated with this download.
    pub fn handle(&self) -> u32 {
        self.handle
    }
}

/// Bounded LRU cache of complete source files for a single camera session.
///
/// The backing [`TempDir`] is created lazily on the first cache miss.  Entries
/// are keyed by camera handle and are only hits when the safe file name,
/// expected length, and on-disk length all still match.  The newest single
/// entry is retained even if it alone exceeds the byte budget; this lets a
/// caller render an unusually large original immediately after downloading
/// it while still bounding all normal multi-entry cache use.
#[derive(Debug)]
pub struct CameraSourceCache {
    temp_dir: Option<Arc<TempDir>>,
    entries: HashMap<u32, CacheEntry>,
    total_bytes: u64,
    max_entries: usize,
    max_bytes: u64,
    use_clock: u64,
    generation: u64,
}

impl Default for CameraSourceCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraSourceCache {
    /// Creates a cache using the production limits (three originals or about
    /// 512 MiB, whichever limit is reached first).
    pub fn new() -> Self {
        Self::with_limits(DEFAULT_MAX_ENTRIES, DEFAULT_MAX_BYTES)
    }

    /// Creates a cache with explicit limits, primarily useful for tests.
    ///
    /// At least one entry is always allowed.  A zero byte limit is accepted,
    /// but the newest single entry is retained so the path returned by
    /// [`Self::commit_write`] remains usable.
    pub fn with_limits(max_entries: usize, max_bytes: u64) -> Self {
        Self {
            temp_dir: None,
            entries: HashMap::new(),
            total_bytes: 0,
            max_entries: max_entries.max(1),
            max_bytes,
            use_clock: 0,
            generation: 1,
        }
    }

    /// Returns the current backing directory without creating it.
    pub fn cache_dir(&self) -> Option<&Path> {
        self.temp_dir.as_ref().map(|directory| directory.path())
    }

    /// Number of valid entries currently tracked by the cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache currently contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Sum of the expected lengths of all tracked entries.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Looks up a complete source file and touches it in the LRU on success.
    ///
    /// A changed file name/size, missing file, symlink, non-regular file, or
    /// unexpected on-disk length invalidates the entry and is reported as a
    /// cache miss rather than an error.
    pub fn lookup(
        &mut self,
        handle: u32,
        filename: &str,
        expected_size: u64,
    ) -> io::Result<Option<PathBuf>> {
        let safe_name = safe_file_name(filename);
        let Some(entry) = self.entries.get(&handle) else {
            return Ok(None);
        };

        let metadata_matches = entry.safe_name == safe_name && entry.expected_size == expected_size;
        let path = entry.path.clone();
        if !metadata_matches || !is_regular_file_of_size(&path, expected_size) {
            self.invalidate(handle)?;
            return Ok(None);
        }

        let tick = self.next_use_tick();
        if let Some(entry) = self.entries.get_mut(&handle) {
            entry.last_used = tick;
        }
        Ok(Some(path))
    }

    /// Reserves safe `.part` and final paths for a complete-object download.
    ///
    /// Any previous entry for the same handle is invalidated first.  Both paths
    /// are inside the stable, lazily-created session temporary directory.
    pub fn begin_write(
        &mut self,
        handle: u32,
        filename: &str,
        expected_size: u64,
    ) -> io::Result<CacheWriteTarget> {
        self.invalidate(handle)?;

        let safe_name = safe_file_name(filename);
        let generation = self.generation;
        let root_guard = self.ensure_cache_root()?.clone();
        let root = root_guard.path().to_path_buf();
        let final_path = root.join(format!("{handle:08X}-{safe_name}"));
        let mut part_name = final_path
            .file_name()
            .map(OsString::from)
            .unwrap_or_else(|| OsString::from(format!("{handle:08X}-object")));
        part_name.push(".part");
        let part_path = root.join(part_name);

        remove_file_if_present(&part_path)?;
        remove_file_if_present(&final_path)?;

        Ok(CacheWriteTarget {
            handle,
            safe_name,
            expected_size,
            generation,
            root,
            root_guard,
            part_path,
            final_path,
        })
    }

    /// Validates and atomically promotes a completed `.part` file.
    ///
    /// The rename stays within one directory/filesystem.  A short file is
    /// rejected with [`io::ErrorKind::UnexpectedEof`]; any other size mismatch
    /// or a non-regular file is rejected with [`io::ErrorKind::InvalidData`].
    pub fn commit_write(&mut self, target: CacheWriteTarget) -> io::Result<PathBuf> {
        self.validate_target(&target)?;

        let metadata = match fs::symlink_metadata(&target.part_path) {
            Ok(metadata) => metadata,
            Err(error) => return Err(error),
        };
        if !metadata.file_type().is_file() {
            let _ = remove_file_if_present(&target.part_path);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "camera cache partial download is not a regular file",
            ));
        }

        let actual_size = metadata.len();
        if actual_size != target.expected_size {
            let _ = remove_file_if_present(&target.part_path);
            let kind = if actual_size < target.expected_size {
                io::ErrorKind::UnexpectedEof
            } else {
                io::ErrorKind::InvalidData
            };
            return Err(io::Error::new(
                kind,
                format!(
                    "camera source length mismatch: expected {} bytes, received {} bytes",
                    target.expected_size, actual_size
                ),
            ));
        }

        // `rename` is atomic because both paths share the cache directory.
        // Remove a stale destination first for Windows, where rename does not
        // replace an existing file.
        remove_file_if_present(&target.final_path)?;
        fs::rename(&target.part_path, &target.final_path)?;

        let tick = self.next_use_tick();
        if let Some(old) = self.entries.remove(&target.handle) {
            self.total_bytes = self.total_bytes.saturating_sub(old.expected_size);
            if old.path != target.final_path {
                let _ = remove_file_if_present(&old.path);
            }
        }
        self.total_bytes = self.total_bytes.saturating_add(target.expected_size);
        self.entries.insert(
            target.handle,
            CacheEntry {
                safe_name: target.safe_name,
                expected_size: target.expected_size,
                path: target.final_path.clone(),
                last_used: tick,
            },
        );
        self.evict_to_limits()?;

        // The just-committed entry is the most recently used and therefore is
        // retained by eviction (including when it alone exceeds max_bytes).
        Ok(target.final_path)
    }

    /// Removes an unfinished `.part` file.  It is safe to call after `clear`.
    pub fn abort_write(&self, target: CacheWriteTarget) -> io::Result<()> {
        remove_file_if_present(&target.part_path)
    }

    /// Invalidates a single camera handle and deletes its cached file.
    pub fn invalidate(&mut self, handle: u32) -> io::Result<()> {
        let Some(entry) = self.entries.remove(&handle) else {
            return Ok(());
        };
        self.total_bytes = self.total_bytes.saturating_sub(entry.expected_size);
        remove_file_if_present(&entry.path)
    }

    /// Drops all published source files and the backing temporary directory.
    ///
    /// The next miss starts a new lazy directory and pending write targets from
    /// the old generation can no longer be committed. An in-flight target owns
    /// a guard that keeps its old staging directory alive just long enough for
    /// blocking camera I/O to finish or abort; dropping that target then removes
    /// the retired directory.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.total_bytes = 0;
        self.temp_dir = None;
        self.generation = self.generation.wrapping_add(1);
    }

    fn ensure_cache_root(&mut self) -> io::Result<&Arc<TempDir>> {
        if self.temp_dir.is_none() {
            self.temp_dir = Some(Arc::new(
                Builder::new()
                    .prefix("nikon-photo-studio-camera-")
                    .tempdir()?,
            ));
        }
        Ok(self.temp_dir.as_ref().expect("cache temp dir initialized"))
    }

    fn validate_target(&self, target: &CacheWriteTarget) -> io::Result<()> {
        let active_root = self.cache_dir();
        let active_root_guard = self.temp_dir.as_ref();
        if target.generation != self.generation
            || active_root != Some(target.root.as_path())
            || !active_root_guard
                .map(|root| Arc::ptr_eq(root, &target.root_guard))
                .unwrap_or(false)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "camera cache write target belongs to an expired session",
            ));
        }
        if target.part_path.parent() != active_root || target.final_path.parent() != active_root {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "camera cache write target escaped its temporary directory",
            ));
        }
        Ok(())
    }

    fn next_use_tick(&mut self) -> u64 {
        self.use_clock = self.use_clock.wrapping_add(1);
        self.use_clock
    }

    fn evict_to_limits(&mut self) -> io::Result<()> {
        loop {
            let over_count = self.entries.len() > self.max_entries;
            // Always retain the newest sole entry so commit_write never returns
            // a path that eviction has immediately deleted.
            let over_bytes = self.total_bytes > self.max_bytes && self.entries.len() > 1;
            if !over_count && !over_bytes {
                return Ok(());
            }

            let Some((&oldest_handle, _)) =
                self.entries.iter().min_by_key(|(_, entry)| entry.last_used)
            else {
                return Ok(());
            };
            self.invalidate(oldest_handle)?;
        }
    }
}

fn is_regular_file_of_size(path: &Path, expected_size: u64) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file() && metadata.len() == expected_size)
        .unwrap_or(false)
}

fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Reduces an untrusted camera-provided path to one portable file name.
fn safe_file_name(filename: &str) -> String {
    // Split both separator styles so this stays safe even if a Windows-style
    // camera path is processed on another platform.
    let leaf = filename
        .rsplit(['/', '\\'])
        .find(|component| !component.is_empty())
        .unwrap_or("camera-object");
    let mut safe: String = leaf
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    safe = safe.trim_end_matches([' ', '.']).to_owned();
    if safe.is_empty() || safe == "." || safe == ".." {
        "camera-object".to_owned()
    } else {
        safe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit_bytes(
        cache: &mut CameraSourceCache,
        handle: u32,
        name: &str,
        bytes: &[u8],
    ) -> PathBuf {
        let target = cache
            .begin_write(handle, name, bytes.len() as u64)
            .expect("begin cache write");
        fs::write(target.part_path(), bytes).expect("write partial source");
        cache.commit_write(target).expect("commit cache write")
    }

    #[test]
    fn second_lookup_hits_same_stable_file() {
        let mut cache = CameraSourceCache::new();
        assert!(cache.cache_dir().is_none(), "directory must be lazy");

        let committed = commit_bytes(&mut cache, 42, "DSC_0042.NEF", b"raw-data");
        let first_root = cache.cache_dir().expect("cache directory").to_path_buf();
        let hit = cache
            .lookup(42, "DSC_0042.NEF", 8)
            .expect("lookup")
            .expect("cache hit");

        assert_eq!(hit, committed);
        assert_eq!(cache.cache_dir(), Some(first_root.as_path()));
        assert_eq!(fs::read(hit).expect("read cached source"), b"raw-data");
    }

    #[test]
    fn changed_size_or_name_invalidates_handle() {
        let mut cache = CameraSourceCache::new();
        let old_path = commit_bytes(&mut cache, 7, "A.NEF", b"1234");
        assert!(cache.lookup(7, "A.NEF", 5).expect("lookup").is_none());
        assert!(!old_path.exists());
        assert!(cache.is_empty());

        let renamed_path = commit_bytes(&mut cache, 7, "A.NEF", b"1234");
        assert!(cache.lookup(7, "B.NEF", 4).expect("lookup").is_none());
        assert!(!renamed_path.exists());
        assert!(cache.is_empty());
    }

    #[test]
    fn short_partial_file_is_rejected_and_removed() {
        let mut cache = CameraSourceCache::new();
        let target = cache
            .begin_write(8, "SHORT.NEF", 5)
            .expect("begin cache write");
        let part_path = target.part_path().to_path_buf();
        fs::write(&part_path, b"1234").expect("write short partial");

        let error = cache
            .commit_write(target)
            .expect_err("short file must fail");
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert!(!part_path.exists());
        assert!(cache.lookup(8, "SHORT.NEF", 5).expect("lookup").is_none());
    }

    #[test]
    fn untrusted_path_is_reduced_to_safe_leaf_name() {
        let mut cache = CameraSourceCache::new();
        let target = cache
            .begin_write(9, r"..\..\outside:photo.NEF", 1)
            .expect("begin cache write");
        let root = cache.cache_dir().expect("cache directory").to_path_buf();

        assert_eq!(target.part_path().parent(), Some(root.as_path()));
        assert_eq!(target.final_path.parent(), Some(root.as_path()));
        let final_name = target
            .final_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("UTF-8 test name");
        assert_eq!(final_name, "00000009-outside_photo.NEF");
        assert!(!final_name.contains(".."));
        assert!(!final_name.contains(['/', '\\', ':']));
    }

    #[test]
    fn clear_drops_directory_and_expires_pending_targets() {
        let mut cache = CameraSourceCache::new();
        let committed = commit_bytes(&mut cache, 1, "ONE.NEF", b"1");
        let old_root = cache.cache_dir().expect("old root").to_path_buf();
        let stale = cache.begin_write(2, "TWO.NEF", 1).expect("pending write");
        fs::write(stale.part_path(), b"2").expect("write pending file");

        cache.clear();
        assert!(cache.cache_dir().is_none());
        assert!(cache.is_empty());
        assert_eq!(cache.total_bytes(), 0);
        assert!(
            old_root.exists(),
            "pending target must keep the retired staging directory alive"
        );
        assert!(committed.exists());
        assert_eq!(
            cache
                .commit_write(stale)
                .expect_err("stale target must fail")
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(
            !old_root.exists(),
            "retired directory should be deleted when its last target drops"
        );

        let fresh = cache.begin_write(3, "THREE.NEF", 1).expect("fresh write");
        assert_ne!(fresh.root, old_root);
    }

    #[test]
    fn concurrent_clear_does_not_remove_an_active_staging_directory() {
        use std::sync::{Arc, Barrier};

        let mut cache = CameraSourceCache::new();
        let target = cache
            .begin_write(11, r"DCIM\ACTIVE.NEF", 4)
            .expect("begin active write");
        let old_root = target.root.clone();
        let part_path = target.part_path().to_path_buf();
        let ready = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        let worker_ready = Arc::clone(&ready);
        let worker_resume = Arc::clone(&resume);
        let worker = std::thread::spawn(move || {
            worker_ready.wait();
            worker_resume.wait();
            fs::write(&part_path, b"data").expect("old transfer can finish staging after clear");
            target
        });

        ready.wait();
        cache.clear();
        assert!(old_root.exists(), "active target keeps old TempDir alive");
        resume.wait();
        let stale = worker.join().expect("join staging writer");
        assert!(stale.part_path().exists());
        assert_eq!(
            cache
                .commit_write(stale)
                .expect_err("retired generation must never publish")
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!old_root.exists());
    }

    #[test]
    fn lru_touch_and_limits_evict_oldest_file() {
        let mut cache = CameraSourceCache::with_limits(2, 100);
        let first = commit_bytes(&mut cache, 1, "ONE.NEF", b"1111");
        let second = commit_bytes(&mut cache, 2, "TWO.NEF", b"2222");
        cache.lookup(1, "ONE.NEF", 4).expect("touch first");
        let third = commit_bytes(&mut cache, 3, "THREE.NEF", b"3333");

        assert!(first.exists(), "touched entry should be retained");
        assert!(!second.exists(), "least-recent entry should be evicted");
        assert!(third.exists());
        assert_eq!(cache.len(), 2);

        // The byte budget independently evicts the older entry.
        let fourth = commit_bytes(&mut cache, 4, "FOUR.NEF", &[4; 98]);
        assert!(fourth.exists());
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.total_bytes(), 98);
    }
}
