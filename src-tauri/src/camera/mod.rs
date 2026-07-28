pub mod commands;
pub mod ptpip;
pub mod usb;

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Camera connection state managed by Tauri
pub struct CameraState {
    pub connection: Mutex<Option<CameraConnection>>,
}

impl Default for CameraState {
    fn default() -> Self {
        Self { connection: Mutex::new(None) }
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
pub struct DownloadProgress {
    pub filename: String,
    pub bytes_received: u64,
    pub total_bytes: u64,
    pub percent: f32,
}
