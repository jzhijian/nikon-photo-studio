pub mod commands;
pub mod preset_probe;
pub mod sdk_probe;
pub mod ptpip;
pub mod source_cache;
pub mod usb;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{atomic::AtomicU64, Arc, Mutex};

const THUMBNAIL_CACHE_MAX_ENTRIES: usize = 512;
const THUMBNAIL_CACHE_MAX_BYTES: usize = 96 * 1024 * 1024;

/// Small per-session LRU used for data URLs returned to the transfer view.
///
/// Camera handles are only meaningful inside one connection, so the cache is
/// cleared whenever a transport is connected/disconnected. Both entry and
/// byte limits are enforced: a card with thousands of photos must not make
/// the desktop process retain every base64 thumbnail forever.
pub struct CameraStringCache {
    entries: HashMap<u32, String>,
    order: VecDeque<u32>,
    bytes: usize,
    max_entries: usize,
    max_bytes: usize,
}

impl CameraStringCache {
    fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            max_entries,
            max_bytes,
        }
    }

    pub fn get_cloned(&mut self, handle: u32) -> Option<String> {
        let value = self.entries.get(&handle)?.clone();
        self.touch(handle);
        Some(value)
    }

    pub fn insert(&mut self, handle: u32, value: String) {
        if value.len() > self.max_bytes {
            return;
        }
        if let Some(previous) = self.entries.remove(&handle) {
            self.bytes = self.bytes.saturating_sub(previous.len());
        }
        self.order.retain(|cached| *cached != handle);
        self.bytes = self.bytes.saturating_add(value.len());
        self.entries.insert(handle, value);
        self.order.push_back(handle);
        self.trim();
    }

    pub fn remove(&mut self, handle: u32) {
        if let Some(previous) = self.entries.remove(&handle) {
            self.bytes = self.bytes.saturating_sub(previous.len());
        }
        self.order.retain(|cached| *cached != handle);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.bytes = 0;
    }

    fn touch(&mut self, handle: u32) {
        self.order.retain(|cached| *cached != handle);
        self.order.push_back(handle);
    }

    fn trim(&mut self) {
        while self.entries.len() > self.max_entries || self.bytes > self.max_bytes {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(value) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(value.len());
            }
        }
    }
}

/// All transport-specific state that describes one published camera session.
///
/// Keeping these values behind one mutex is important: a USB catalog must
/// never be observed without its matching USB connection, and a Wi-Fi
/// connection must never be observed without its matching PTP/IP client.
pub struct CameraSession {
    pub generation: u64,
    pub connection: Option<CameraConnection>,
    /// PTP/IP itself is strictly serial, but its network I/O must not hold the
    /// session mutex. Keeping a separate shared client lock lets a new
    /// connect/disconnect retire the published session immediately while an
    /// older scan or preview is blocked in the camera responder.
    pub wifi_client: Option<Arc<Mutex<ptpip::PtpIpClient>>>,
    /// Socket clones used only to interrupt a blocking PTP/IP transaction.
    /// This handle never needs the client mutex, so disconnect remains prompt.
    pub wifi_abort: Option<ptpip::PtpIpAbortHandle>,
    pub usb_catalog: Vec<usb::UsbPhotoRecord>,
    /// Metadata enumeration is expensive over Wi-Fi (one GetObjectInfo per
    /// handle). Cache the completed catalog for this connection.
    pub wifi_catalog: Vec<CameraPhoto>,
    pub wifi_catalog_loaded: bool,
    pub thumbnail_cache: CameraStringCache,
    /// Complete camera originals retained for the current connection.  This
    /// lets the detail viewer request progressively larger renders without
    /// transferring the same NEF again for every zoom bucket.
    pub source_cache: source_cache::CameraSourceCache,
    /// Serializes complete-original materialization inside this connection
    /// generation. A connection reset replaces the Arc instead of waiting for
    /// an old, slow RAW transfer, so the new generation can start immediately.
    pub source_io: Arc<Mutex<()>>,
}

impl Default for CameraSession {
    fn default() -> Self {
        Self {
            generation: 0,
            connection: None,
            wifi_client: None,
            wifi_abort: None,
            usb_catalog: Vec::new(),
            wifi_catalog: Vec::new(),
            wifi_catalog_loaded: false,
            thumbnail_cache: CameraStringCache::new(
                THUMBNAIL_CACHE_MAX_ENTRIES,
                THUMBNAIL_CACHE_MAX_BYTES,
            ),
            source_cache: source_cache::CameraSourceCache::new(),
            source_io: Arc::new(Mutex::new(())),
        }
    }
}

impl CameraSession {
    pub fn clear_cached_camera_data(&mut self) {
        self.wifi_catalog.clear();
        self.wifi_catalog_loaded = false;
        self.thumbnail_cache.clear();
        self.source_cache.clear();
        self.source_io = Arc::new(Mutex::new(()));
    }
}

/// Camera connection state managed by Tauri
pub struct CameraState {
    /// Monotonically increasing ownership token. It is separate from the
    /// session mutex so a new connect/disconnect request invalidates an older
    /// blocking scan immediately, before that scan gets a chance to publish.
    pub generation: AtomicU64,
    pub session: Mutex<CameraSession>,
    /// Windows Portable Devices serializes access to a camera. Keep shell
    /// thumbnail, preview, copy and delete operations from racing each other.
    pub usb_io: Mutex<()>,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            generation: AtomicU64::new(0),
            session: Mutex::new(CameraSession::default()),
            usb_io: Mutex::new(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionType {
    WifiAP,
    WifiSTA,
    USB,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraConnection {
    pub conn_type: ConnectionType,
    pub host: String,
    pub port: u16,
    pub camera_model: String,
    pub session_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraPhoto {
    pub handle: u32,
    pub filename: String,
    pub size: u64,
    pub format: PhotoFormat,
    pub date: String,
    pub width: u32,
    pub height: u32,
    pub source_path: Option<String>,
    /// True when the camera reports that the object data is protected from
    /// deletion. Live camera scans always populate this from ObjectInfo.
    #[serde(default)]
    pub protected: bool,
    /// Whether the object is eligible for an explicit delete request. The
    /// camera can still reject the operation later if the card becomes
    /// read-only or its state changes between enumeration and deletion. Older
    /// serialized records default to false so missing capability information
    /// never enables a destructive action.
    #[serde(default)]
    pub can_delete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PhotoFormat {
    JPEG,
    NEF,
    TIFF,
    Unknown(u16),
}

impl From<u16> for PhotoFormat {
    fn from(code: u16) -> Self {
        match code {
            0x3801 => PhotoFormat::JPEG,
            0xB103 | 0xB108 => PhotoFormat::NEF,
            0x380D => PhotoFormat::TIFF,
            other => PhotoFormat::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub handle: u32,
    pub filename: String,
    pub bytes_received: u64,
    pub total_bytes: u64,
    pub percent: f32,
}

#[cfg(test)]
mod tests {
    use super::{CameraSession, CameraStringCache};
    use std::sync::Arc;

    #[test]
    fn bounded_camera_cache_evicts_lru_by_entry_count() {
        let mut cache = CameraStringCache::new(2, 1024);
        cache.insert(1, "one".into());
        cache.insert(2, "two".into());
        assert_eq!(cache.get_cloned(1).as_deref(), Some("one"));
        cache.insert(3, "three".into());

        assert!(cache.get_cloned(2).is_none());
        assert_eq!(cache.get_cloned(1).as_deref(), Some("one"));
        assert_eq!(cache.get_cloned(3).as_deref(), Some("three"));
    }

    #[test]
    fn bounded_camera_cache_enforces_byte_limit_and_remove() {
        let mut cache = CameraStringCache::new(8, 6);
        cache.insert(1, "1234".into());
        cache.insert(2, "5678".into());
        assert!(cache.get_cloned(1).is_none());
        assert_eq!(cache.get_cloned(2).as_deref(), Some("5678"));
        cache.remove(2);
        assert!(cache.get_cloned(2).is_none());
    }

    #[test]
    fn resetting_session_replaces_source_io_without_waiting_for_old_generation() {
        let mut session = CameraSession::default();
        let retired_io = Arc::clone(&session.source_io);
        let _retired_guard = retired_io.lock().expect("lock retired source generation");

        session.clear_cached_camera_data();
        assert!(!Arc::ptr_eq(&retired_io, &session.source_io));
        assert!(
            session.source_io.try_lock().is_ok(),
            "new generation must not wait for a RAW transfer on the retired lock"
        );
    }
}
