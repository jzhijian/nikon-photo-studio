//! Tauri command handlers for camera module

use serde::{Deserialize, Serialize};
use tauri::State;
use super::ptpip::{PtpIpClient, DEFAULT_AP_HOST, DEFAULT_PTPIP_PORT};
use super::{CameraConnection, CameraPhoto, CameraState, ConnectionType, DownloadProgress};

#[derive(Debug, Deserialize)]
pub struct ConnectWifiArgs {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub mode: String, // "ap" or "sta"
}

#[derive(Debug, Serialize)]
pub struct ConnectResult {
    pub success: bool,
    pub message: String,
    pub camera_model: Option<String>,
}

#[tauri::command]
pub async fn connect_wifi(
    args: ConnectWifiArgs,
    state: State<'_, CameraState>,
) -> Result<ConnectResult, String> {
    let host = args.host.unwrap_or_else(|| DEFAULT_AP_HOST.to_string());
    let port = args.port.unwrap_or(DEFAULT_PTPIP_PORT);

    let mut client = PtpIpClient::connect(&host, port)
        .map_err(|e| format!("Connection failed: {}", e))?;

    let session_id = client.open_session()
        .map_err(|e| format!("Session failed: {}", e))?;

    let conn = CameraConnection {
        conn_type: if args.mode == "sta" { ConnectionType::WifiSTA } else { ConnectionType::WifiAP },
        host: host.clone(),
        port,
        camera_model: "Nikon Z30".to_string(), // TODO: get from DeviceInfo
        session_id,
    };

    *state.connection.lock().unwrap() = Some(conn);

    Ok(ConnectResult {
        success: true,
        message: format!("Connected to {}:{} (session {})", host, port, session_id),
        camera_model: Some("Nikon Z30".to_string()),
    })
}

#[tauri::command]
pub async fn connect_usb(state: State<'_, CameraState>) -> Result<ConnectResult, String> {
    let devices = super::usb::UsbCamera::scan_devices()
        .map_err(|e| format!("USB scan failed: {}", e))?;

    if devices.is_empty() {
        return Ok(ConnectResult {
            success: false,
            message: "No Nikon camera found via USB".to_string(),
            camera_model: None,
        });
    }

    let (path, name) = &devices[0];
    let _camera = super::usb::UsbCamera::connect(path)
        .map_err(|e| format!("USB connect failed: {}", e))?;

    let conn = CameraConnection {
        conn_type: ConnectionType::USB,
        host: path.clone(),
        port: 0,
        camera_model: name.clone(),
        session_id: 1,
    };

    *state.connection.lock().unwrap() = Some(conn);

    Ok(ConnectResult {
        success: true,
        message: format!("Connected via USB: {}", name),
        camera_model: Some(name.clone()),
    })
}

#[tauri::command]
pub async fn disconnect(state: State<'_, CameraState>) -> Result<String, String> {
    *state.connection.lock().unwrap() = None;
    Ok("Disconnected".to_string())
}

#[tauri::command]
pub async fn list_photos(state: State<'_, CameraState>) -> Result<Vec<CameraPhoto>, String> {
    let conn = state.connection.lock().unwrap();
    let conn = conn.as_ref().ok_or("Not connected")?;

    let mut client = PtpIpClient::connect(&conn.host, conn.port)
        .map_err(|e| format!("Reconnect failed: {}", e))?;
    client.open_session().map_err(|e| e.to_string())?;

    let handles = client.get_object_handles().map_err(|e| e.to_string())?;
    let mut photos = Vec::new();
    for handle in handles.iter().take(500) {
        if let Ok(info) = client.get_object_info(*handle) {
            photos.push(info);
        }
    }
    Ok(photos)
}

#[tauri::command]
pub async fn download_photo(
    handle: u32,
    dest_path: String,
    state: State<'_, CameraState>,
) -> Result<String, String> {
    let conn = state.connection.lock().unwrap();
    let conn = conn.as_ref().ok_or("Not connected")?;

    let mut client = PtpIpClient::connect(&conn.host, conn.port)
        .map_err(|e| format!("Reconnect failed: {}", e))?;
    client.open_session().map_err(|e| e.to_string())?;

    let data = client.get_object(handle).map_err(|e| e.to_string())?;
    std::fs::write(&dest_path, &data).map_err(|e| format!("Write failed: {}", e))?;

    Ok(dest_path)
}

#[tauri::command]
pub async fn get_thumbnail(
    handle: u32,
    state: State<'_, CameraState>,
) -> Result<Vec<u8>, String> {
    let conn = state.connection.lock().unwrap();
    let conn = conn.as_ref().ok_or("Not connected")?;

    let mut client = PtpIpClient::connect(&conn.host, conn.port)
        .map_err(|e| format!("Reconnect failed: {}", e))?;
    client.open_session().map_err(|e| e.to_string())?;

    client.get_thumbnail(handle).map_err(|e| e.to_string())
}
