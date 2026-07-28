//! USB MTP/PTP connection for Nikon cameras.
//! Uses Windows WPD (Windows Portable Devices) COM API or libusb.

use super::{CameraPhoto, PhotoFormat};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum UsbError {
    #[error("No Nikon camera found")]
    NoDevice,
    #[error("USB connection failed: {0}")]
    Connection(String),
    #[error("Transfer error: {0}")]
    Transfer(String),
}

/// Nikon USB Vendor ID
pub const NIKON_VID: u16 = 0x04B0;

/// Known Nikon Product IDs
pub const NIKON_PIDS: &[(u16, &str)] = &[
    (0x043A, "Nikon Z30"),
    (0x043F, "Nikon Z5II"),
    (0x0436, "Nikon Z6II"),
    (0x0437, "Nikon Z7II"),
    (0x0439, "Nikon Z9"),
    (0x043B, "Nikon Z fc"),
];

pub struct UsbCamera {
    device_name: String,
    connected: bool,
}

impl UsbCamera {
    /// Scan for connected Nikon cameras via USB
    pub fn scan_devices() -> Result<Vec<(String, String)>, UsbError> {
        // TODO: Implement WPD COM API enumeration
        // For now, return empty list - will be implemented with windows-rs crate
        Ok(vec![])
    }

    /// Connect to a Nikon camera via USB MTP/PTP
    pub fn connect(device_path: &str) -> Result<Self, UsbError> {
        // TODO: Open WPD device and establish PTP session
        Ok(Self {
            device_name: device_path.to_string(),
            connected: true,
        })
    }

    /// List photos on the camera
    pub fn list_photos(&self) -> Result<Vec<CameraPhoto>, UsbError> {
        // TODO: Enumerate objects via MTP/PTP
        Ok(vec![])
    }

    /// Download a photo by handle
    pub fn download(&self, _handle: u32) -> Result<Vec<u8>, UsbError> {
        // TODO: Get object via MTP/PTP
        Ok(vec![])
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nikon_vid() {
        assert_eq!(NIKON_VID, 0x04B0);
    }

    #[test]
    fn test_known_pids() {
        let z30 = NIKON_PIDS.iter().find(|(pid, _)| *pid == 0x043A);
        assert!(z30.is_some());
        assert_eq!(z30.unwrap().1, "Nikon Z30");
    }
}
