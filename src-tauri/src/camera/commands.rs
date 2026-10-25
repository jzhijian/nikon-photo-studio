//! Tauri command handlers for camera module

use super::ptpip::{PtpIpClient, DEFAULT_AP_HOST, DEFAULT_PTPIP_PORT};
use super::{
    CameraConnection, CameraPhoto, CameraState, ConnectionType, DownloadProgress, PhotoFormat,
};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{atomic::Ordering, Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

fn background_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    command
}

const SUPERSEDED: &str = "相机连接请求已被更新的连接或断开操作替代";

/// Claim ownership of the next camera session and atomically retire whatever
/// transport was previously published. The atomic generation changes before
/// taking the session lock, so a slow in-flight connect can already see that
/// it no longer owns the right to publish.
fn begin_connection(state: &CameraState) -> Result<u64, String> {
    let generation = state
        .generation
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    let (previous_client, previous_abort) = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        if state.generation.load(Ordering::Acquire) != generation {
            return Err(SUPERSEDED.to_string());
        }
        session.generation = generation;
        session.connection = None;
        session.usb_catalog.clear();
        session.clear_cached_camera_data();
        (session.wifi_client.take(), session.wifi_abort.take())
    };
    // PTP/IP Drop may perform CloseSession I/O; never do that while holding
    // the state mutex needed by a newer request.
    if let Some(abort) = previous_abort {
        abort.abort();
    }
    drop(previous_client);
    Ok(generation)
}

fn session_is_current(state: &CameraState, generation: u64) -> bool {
    state.generation.load(Ordering::Acquire) == generation
}

fn ensure_active_session(state: &CameraState, generation: u64) -> Result<(), String> {
    if session_is_current(state, generation) {
        Ok(())
    } else {
        Err(SUPERSEDED.to_string())
    }
}

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetworkInfo {
    pub ssid: Option<String>,
    pub local_ipv4: Option<String>,
    pub gateway: Option<String>,
    pub looks_like_nikon_ap: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraThumbnail {
    pub handle: u32,
    pub url: String,
}

/// A complete camera object made available through the session source cache.
/// The frontend passes `path` to the same adaptive renderer used by Library
/// and Organize, so RAW detail quality follows the visible area, display DPI,
/// and zoom level instead of being locked to one preview size.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedCameraPhoto {
    pub path: String,
    pub handle: u32,
    pub filename: String,
    pub size: u64,
    pub format: PhotoFormat,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize)]
pub struct DeleteCameraResult {
    pub deleted: Vec<u32>,
    pub protected: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraConnectionProgress {
    pub attempt: u8,
    pub total_attempts: u8,
    pub phase: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraScanProgress {
    pub scanned: usize,
    pub total: usize,
    pub loaded: usize,
    pub phase: &'static str,
}

fn emit_connection_progress(
    app: &AppHandle,
    attempt: u8,
    phase: &'static str,
    message: impl Into<String>,
) {
    emit_connection_progress_with_total(app, attempt, 6, phase, message);
}

fn emit_connection_progress_with_total(
    app: &AppHandle,
    attempt: u8,
    total_attempts: u8,
    phase: &'static str,
    message: impl Into<String>,
) {
    let _ = app.emit(
        "camera-connection-progress",
        connection_progress_payload(attempt, total_attempts, phase, message),
    );
}

fn connection_progress_payload(
    attempt: u8,
    total_attempts: u8,
    phase: &'static str,
    message: impl Into<String>,
) -> CameraConnectionProgress {
    CameraConnectionProgress {
        attempt,
        total_attempts,
        phase,
        message: message.into(),
    }
}

fn emit_scan_progress(
    app: &AppHandle,
    scanned: usize,
    total: usize,
    loaded: usize,
    phase: &'static str,
) {
    let _ = app.emit(
        "camera-scan-progress",
        CameraScanProgress {
            scanned,
            total,
            loaded,
            phase,
        },
    );
}

fn emit_download_progress(
    app: &AppHandle,
    handle: u32,
    filename: &str,
    bytes_received: u64,
    total_bytes: u64,
) {
    let percent = if total_bytes == 0 {
        0.0
    } else {
        (bytes_received as f64 * 100.0 / total_bytes as f64).min(100.0) as f32
    };
    let _ = app.emit(
        "camera-download-progress",
        DownloadProgress {
            handle,
            filename: filename.to_string(),
            bytes_received,
            total_bytes,
            percent,
        },
    );
}

fn emit_preview_progress(
    app: &AppHandle,
    handle: u32,
    filename: &str,
    bytes_received: u64,
    total_bytes: u64,
) {
    let percent = if total_bytes == 0 {
        0.0
    } else {
        (bytes_received as f64 * 100.0 / total_bytes as f64).min(100.0) as f32
    };
    let _ = app.emit(
        "camera-preview-progress",
        DownloadProgress {
            handle,
            filename: filename.to_string(),
            bytes_received,
            total_bytes,
            percent,
        },
    );
}

/// Materialize one complete camera original into the session-scoped source
/// cache.  PTP/IP remains strictly serial, while the 64 KiB streaming writer
/// avoids a second RAW-sized allocation.  USB uses the same staging/length
/// validation even though Windows performs the underlying MTP copy.
fn prepare_camera_source(
    handle: u32,
    app: &AppHandle,
    state: &CameraState,
) -> Result<(CameraPhoto, PathBuf), String> {
    // Complete originals are serialized per connection generation. Resetting a
    // session replaces this Arc, so a new connection never waits for a slow RAW
    // still finishing on the retired transport.
    let (requested_generation, source_io) = {
        let session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        (
            session.generation,
            std::sync::Arc::clone(&session.source_io),
        )
    };
    let _source_io = source_io
        .lock()
        .map_err(|_| "Camera source I/O lock is unavailable")?;
    ensure_active_session(state, requested_generation)?;

    let (generation, photo, usb_record, wifi_client, target) = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        let generation = session.generation;
        ensure_active_session(state, generation)?;
        let is_usb = session
            .connection
            .as_ref()
            .map(|connection| matches!(connection.conn_type, ConnectionType::USB))
            .ok_or("Not connected")?;

        let (photo, usb_record) = if is_usb {
            let record = session
                .usb_catalog
                .iter()
                .find(|record| record.photo.handle == handle)
                .cloned()
                .ok_or_else(|| format!("USB photo handle {handle} not found"))?;
            (record.photo.clone(), Some(record))
        } else {
            // The transfer grid is only published after metadata enumeration;
            // do not fall back to PTP/IP while holding CameraSession's mutex.
            let photo = session
                .wifi_catalog
                .iter()
                .find(|photo| photo.handle == handle)
                .cloned()
                .ok_or_else(|| {
                    format!("Wi-Fi photo handle {handle} not found; rescan the camera")
                })?;
            (photo, None)
        };

        if photo.size == 0 {
            return Err(format!(
                "相机没有报告 {} 的原文件大小，无法安全验证高清预览",
                photo.filename
            ));
        }
        if let Some(path) = session
            .source_cache
            .lookup(handle, &photo.filename, photo.size)
            .map_err(|error| format!("Unable to inspect camera preview cache: {error}"))?
        {
            return Ok((photo, path));
        }
        let target = session
            .source_cache
            .begin_write(handle, &photo.filename, photo.size)
            .map_err(|error| format!("Unable to prepare camera preview cache: {error}"))?;
        let wifi_client = if is_usb {
            None
        } else {
            Some(
                session
                    .wifi_client
                    .as_ref()
                    .cloned()
                    .ok_or("Wi-Fi session is not active; reconnect the camera")?,
            )
        };
        (generation, photo, usb_record, wifi_client, target)
    };

    emit_preview_progress(app, handle, &photo.filename, 0, photo.size);
    let transfer_result: Result<u64, String> = (|| {
        if let Some(record) = usb_record {
            let _io = state
                .usb_io
                .lock()
                .map_err(|_| "USB I/O lock is unavailable")?;
            ensure_active_session(state, generation)?;
            super::usb::UsbCamera::download(
                &record,
                target
                    .part_path()
                    .to_str()
                    .ok_or("camera preview cache path is not UTF-8")?,
            )
            .map_err(|error| error.to_string())?;
            Ok(photo.size)
        } else {
            let mut file = File::create(target.part_path())
                .map_err(|error| format!("Unable to create camera preview cache file: {error}"))?;
            ensure_active_session(state, generation)?;
            let mut client = wifi_client
                .as_ref()
                .ok_or("Wi-Fi preview client is unavailable; reconnect the camera")?
                .lock()
                .map_err(|_| "Wi-Fi PTP/IP client lock is unavailable")?;
            // A thumbnail may have owned the serial PTP connection while this
            // detail request was queued. Do not start old-generation RAW I/O
            // after a transport switch that occurred during that wait.
            ensure_active_session(state, generation)?;
            let mut last_emitted = 0u64;
            let received = client
                .get_object_to_writer(handle, &mut file, |bytes_received, total_bytes| {
                    const EVENT_STEP: u64 = 512 * 1024;
                    if bytes_received == total_bytes
                        || bytes_received.saturating_sub(last_emitted) >= EVENT_STEP
                    {
                        emit_preview_progress(
                            app,
                            handle,
                            &photo.filename,
                            bytes_received,
                            total_bytes,
                        );
                        last_emitted = bytes_received;
                    }
                })
                .map_err(|error| error.to_string())?;
            file.flush()
                .map_err(|error| format!("Unable to flush camera preview cache: {error}"))?;
            Ok(received)
        }
    })();

    let received = match transfer_result {
        Ok(received) => received,
        Err(error) => {
            if let Ok(session) = state.session.lock() {
                let _ = session.source_cache.abort_write(target);
            }
            return Err(error);
        }
    };
    if let Err(error) = ensure_active_session(state, generation) {
        if let Ok(session) = state.session.lock() {
            let _ = session.source_cache.abort_write(target);
        }
        return Err(error);
    }
    if received != photo.size {
        let error = format!(
            "Camera preview byte count mismatch for {}: {received} / {}",
            photo.filename, photo.size
        );
        if let Ok(session) = state.session.lock() {
            let _ = session.source_cache.abort_write(target);
        }
        return Err(error);
    }

    let path = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        ensure_active_session(state, generation)?;
        if session.generation != generation {
            let _ = std::fs::remove_file(target.part_path());
            return Err(SUPERSEDED.to_string());
        }
        session
            .source_cache
            .commit_write(target)
            .map_err(|error| format!("Unable to publish camera preview cache: {error}"))?
    };
    emit_preview_progress(app, handle, &photo.filename, received, received);
    Ok((photo, path))
}

fn parse_wifi_ssid(netsh: &str) -> Option<String> {
    netsh
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            let (key, value) = trimmed.split_once(':')?;
            let key = key.trim().to_ascii_lowercase();
            if key == "ssid" {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
        .filter(|value| !value.is_empty())
}

fn active_wifi_info() -> WifiNetworkInfo {
    let netsh = background_command("netsh")
        .args(["wlan", "show", "interfaces"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let ssid = parse_wifi_ssid(&netsh);

    let network = background_command("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
            "$c=Get-NetIPConfiguration | Where-Object {$_.NetAdapter.Status -eq 'Up' -and $_.IPv4Address -and $_.InterfaceAlias -match 'Wi-Fi|WLAN|无线'} | Select-Object -First 1; if($c){[pscustomobject]@{ip=$c.IPv4Address.IPAddress;gateway=$c.IPv4DefaultGateway.NextHop}|ConvertTo-Json -Compress}"])
        .output().ok().and_then(|output| serde_json::from_slice::<serde_json::Value>(&output.stdout).ok());
    let local_ipv4 = network
        .as_ref()
        .and_then(|value| value.get("ip"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let gateway = network
        .as_ref()
        .and_then(|value| value.get("gateway"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let looks_like_nikon_ap = ssid
        .as_ref()
        .map(|value| value.to_ascii_uppercase().contains("NIKON"))
        .unwrap_or(false);
    WifiNetworkInfo {
        ssid,
        local_ipv4,
        gateway,
        looks_like_nikon_ap,
    }
}

#[tauri::command]
pub async fn get_wifi_network_info() -> Result<WifiNetworkInfo, String> {
    tauri::async_runtime::spawn_blocking(active_wifi_info)
        .await
        .map_err(|error| format!("Wi-Fi network detection worker failed: {error}"))
}

#[tauri::command]
pub async fn connect_wifi(args: ConnectWifiArgs, app: AppHandle) -> Result<ConnectResult, String> {
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let event_app = worker_app.clone();
        let state = worker_app.state::<CameraState>();
        connect_wifi_blocking(args, event_app, state)
    })
    .await
    .map_err(|error| format!("Wi-Fi connection worker failed: {error}"))?
}

fn connect_wifi_blocking(
    args: ConnectWifiArgs,
    app: AppHandle,
    state: State<'_, CameraState>,
) -> Result<ConnectResult, String> {
    let generation = begin_connection(&state)?;
    let network = active_wifi_info();
    let requested_host = args.host.filter(|value| !value.trim().is_empty());
    let host = if args.mode == "ap" {
        network
            .gateway
            .clone()
            .or(requested_host)
            .unwrap_or_else(|| DEFAULT_AP_HOST.to_string())
    } else {
        requested_host.ok_or_else(|| "STA mode requires the camera IPv4 address".to_string())?
    };
    let port = args.port.unwrap_or(DEFAULT_PTPIP_PORT);

    let mut last_error = None;
    let mut connected_client = None;
    let mut session_id = None;
    let mut connected_attempt = 1u8;
    // Windows may report the NIKON_ SSID before the camera has opened its
    // complete PTP/IP command + event service. Retry the full handshake and
    // OpenSession sequence instead of retrying only the first TCP socket.
    for attempt in 0..6 {
        ensure_active_session(&state, generation)?;
        let attempt_number = (attempt + 1) as u8;
        emit_connection_progress(
            &app,
            attempt_number,
            "transport",
            format!("正在连接 {host}:{port} 并建立 PTP/IP 命令与事件通道"),
        );
        match PtpIpClient::connect(&host, port) {
            Ok(mut client) => {
                emit_connection_progress(
                    &app,
                    attempt_number,
                    "session",
                    "命令与事件双通道已建立，正在打开相机会话",
                );
                match client.open_session() {
                    Ok(opened_session_id) => {
                        session_id = Some(opened_session_id);
                        connected_attempt = attempt_number;
                        connected_client = Some(client);
                        break;
                    }
                    Err(error) => {
                        last_error = Some(format!("PTP/IP session initialization failed: {error}"));
                    }
                }
            }
            Err(error) => {
                last_error = Some(error.to_string());
            }
        }
        if attempt < 5 {
            std::thread::sleep(Duration::from_millis(900));
            if !session_is_current(&state, generation) {
                return Err(SUPERSEDED.to_string());
            }
        }
    }
    let mut client = connected_client.ok_or_else(|| {
        format!(
            "无法建立相机 PTP/IP 会话 {}:{}：{}",
            host,
            port,
            last_error.unwrap_or_else(|| "连接失败".to_string())
        )
    })?;
    let session_id = session_id.ok_or_else(|| "相机没有返回有效的会话编号".to_string())?;
    emit_connection_progress(
        &app,
        connected_attempt,
        "device-info",
        "相机会话已建立，正在读取设备型号",
    );
    let camera_model = client
        .get_device_model()
        .unwrap_or_else(|_| "Nikon camera".to_string());
    let mut abort_handle = Some(
        client
            .abort_handle()
            .map_err(|error| format!("Unable to prepare PTP/IP cancellation: {error}"))?,
    );
    let shared_client = Arc::new(Mutex::new(client));

    let conn = CameraConnection {
        conn_type: if args.mode == "sta" {
            ConnectionType::WifiSTA
        } else {
            ConnectionType::WifiAP
        },
        host: host.clone(),
        port,
        camera_model: camera_model.clone(),
        session_id,
    };

    let (previous_client, previous_abort) = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        if !session_is_current(&state, generation) || session.generation != generation {
            drop(session);
            if let Some(abort) = abort_handle.take() {
                abort.abort();
            }
            drop(shared_client);
            return Err(SUPERSEDED.to_string());
        }
        session.connection = Some(conn);
        session.usb_catalog.clear();
        session.clear_cached_camera_data();
        let previous_client = session.wifi_client.replace(shared_client);
        let previous_abort = std::mem::replace(&mut session.wifi_abort, abort_handle.take());
        (previous_client, previous_abort)
    };
    if let Some(abort) = previous_abort {
        abort.abort();
    }
    drop(previous_client);
    emit_connection_progress(
        &app,
        connected_attempt,
        "complete",
        format!("已连接 {camera_model}"),
    );

    Ok(ConnectResult {
        success: true,
        message: format!("Connected to {}:{} (session {})", host, port, session_id),
        camera_model: Some(camera_model),
    })
}

#[tauri::command]
pub async fn connect_usb(app: AppHandle) -> Result<ConnectResult, String> {
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let event_app = worker_app.clone();
        let state = worker_app.state::<CameraState>();
        connect_usb_blocking(event_app, state)
    })
    .await
    .map_err(|error| format!("USB camera scan worker failed: {error}"))?
}

fn connect_usb_blocking(
    app: AppHandle,
    state: State<'_, CameraState>,
) -> Result<ConnectResult, String> {
    let generation = begin_connection(&state)?;
    emit_connection_progress_with_total(
        &app,
        1,
        1,
        "usb-detect",
        "正在检测通过 USB（MTP/PTP）连接的 Nikon 相机",
    );
    emit_scan_progress(&app, 0, 0, 0, "usb-detect");
    let devices = super::usb::UsbCamera::scan_devices().map_err(|error| {
        emit_connection_progress_with_total(
            &app,
            1,
            1,
            "error",
            format!("USB 设备检测失败：{error}"),
        );
        format!("USB scan failed: {error}")
    })?;
    ensure_active_session(&state, generation)?;

    if devices.is_empty() {
        emit_connection_progress_with_total(&app, 1, 1, "error", "未检测到可用的 Nikon USB 相机");
        emit_scan_progress(&app, 0, 0, 0, "complete");
        return Ok(ConnectResult {
            success: false,
            message: "No Nikon camera found via USB".to_string(),
            camera_model: None,
        });
    }

    let (path, name) = &devices[0];
    emit_connection_progress_with_total(
        &app,
        1,
        1,
        "usb-storage",
        format!("已检测到 {name}，正在等待存储卡并读取照片目录"),
    );
    emit_scan_progress(&app, 0, 0, 0, "usb-storage");
    let camera = {
        let _io = state
            .usb_io
            .lock()
            .map_err(|_| "USB I/O lock is unavailable")?;
        super::usb::UsbCamera::connect(path).map_err(|error| {
            emit_connection_progress_with_total(
                &app,
                1,
                1,
                "error",
                format!("无法读取 {name} 的存储卡：{error}"),
            );
            format!("USB connect failed: {error}")
        })?
    };
    let records = camera.records().to_vec();
    let photo_count = records.len();
    ensure_active_session(&state, generation)?;
    emit_scan_progress(&app, photo_count, photo_count, photo_count, "complete");
    emit_connection_progress_with_total(
        &app,
        1,
        1,
        "usb-enumerated",
        format!("存储卡已就绪，已枚举 {photo_count} 张照片，正在建立传输会话"),
    );

    let conn = CameraConnection {
        conn_type: ConnectionType::USB,
        host: path.clone(),
        port: 0,
        camera_model: name.clone(),
        session_id: 1,
    };

    let (previous_client, previous_abort) = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        if !session_is_current(&state, generation) || session.generation != generation {
            return Err(SUPERSEDED.to_string());
        }
        session.connection = Some(conn);
        session.usb_catalog = records;
        session.clear_cached_camera_data();
        (session.wifi_client.take(), session.wifi_abort.take())
    };
    if let Some(abort) = previous_abort {
        abort.abort();
    }
    drop(previous_client);
    emit_connection_progress_with_total(
        &app,
        1,
        1,
        "complete",
        format!("已连接 {name}，读取到 {photo_count} 张照片"),
    );

    Ok(ConnectResult {
        success: true,
        message: format!("Connected via USB: {}", name),
        camera_model: Some(name.clone()),
    })
}

#[tauri::command]
pub async fn disconnect(state: State<'_, CameraState>) -> Result<String, String> {
    let generation = state
        .generation
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    let (previous_client, previous_abort) = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        // A newer request wins. An older disconnect must never clear the
        // session published (or being prepared) by that newer request.
        if !session_is_current(&state, generation) {
            return Ok("Disconnect superseded by a newer camera request".to_string());
        }
        session.generation = generation;
        session.connection = None;
        session.usb_catalog.clear();
        session.clear_cached_camera_data();
        (session.wifi_client.take(), session.wifi_abort.take())
    };
    if let Some(abort) = previous_abort {
        abort.abort();
    }
    drop(previous_client);
    Ok("Disconnected".to_string())
}

#[tauri::command]
pub async fn list_photos(app: AppHandle) -> Result<Vec<CameraPhoto>, String> {
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let event_app = worker_app.clone();
        let state = worker_app.state::<CameraState>();
        list_photos_blocking(event_app, state)
    })
    .await
    .map_err(|error| format!("Camera catalog worker failed: {error}"))?
}

fn list_photos_blocking(
    app: AppHandle,
    state: State<'_, CameraState>,
) -> Result<Vec<CameraPhoto>, String> {
    let (generation, shared_client) = {
        let session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        let generation = session.generation;
        ensure_active_session(&state, generation)?;
        let is_usb = session
            .connection
            .as_ref()
            .map(|connection| matches!(connection.conn_type, ConnectionType::USB))
            .ok_or("Not connected")?;
        if is_usb {
            return Ok(session
                .usb_catalog
                .iter()
                .map(|record| record.photo.clone())
                .collect());
        }
        if session.wifi_catalog_loaded {
            let cached = session.wifi_catalog.clone();
            emit_scan_progress(&app, cached.len(), cached.len(), cached.len(), "cached");
            return Ok(cached);
        }
        let client = session
            .wifi_client
            .as_ref()
            .cloned()
            .ok_or("Wi-Fi session is not active; reconnect the camera")?;
        (generation, client)
    };

    // PTP transactions remain serial on their own lock. The session lock is
    // deliberately not held here, allowing disconnect to take the abort
    // handle and shutdown this socket while a camera read is pending.
    let mut client = shared_client
        .lock()
        .map_err(|_| "Wi-Fi PTP/IP client lock is unavailable")?;
    ensure_active_session(&state, generation)?;
    let handles = client.get_object_handles().map_err(|e| e.to_string())?;
    let total = handles.len();
    emit_scan_progress(&app, 0, total, 0, "metadata");
    let mut photos = Vec::with_capacity(total);
    for (index, handle) in handles.iter().enumerate() {
        if let Ok(info) = client.get_object_info(*handle) {
            photos.push(info);
        }
        let scanned = index + 1;
        if scanned % 20 == 0 || scanned == total {
            ensure_active_session(&state, generation)?;
            emit_scan_progress(&app, scanned, total, photos.len(), "metadata");
        }
    }
    ensure_active_session(&state, generation)?;
    drop(client);
    let mut session = state
        .session
        .lock()
        .map_err(|_| "Camera session lock is unavailable")?;
    if session.generation != generation {
        return Err(SUPERSEDED.to_string());
    }
    session.wifi_catalog = photos.clone();
    session.wifi_catalog_loaded = true;
    emit_scan_progress(&app, total, total, photos.len(), "complete");
    Ok(photos)
}

#[tauri::command]
pub async fn download_photo(
    handle: u32,
    dest_path: String,
    app: AppHandle,
    state: State<'_, CameraState>,
) -> Result<String, String> {
    // If this object was already materialized for the adaptive detail viewer,
    // publish that exact original locally instead of asking the camera for the
    // same 20-30 MB RAW a second time.
    let cached_source = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        let generation = session.generation;
        ensure_active_session(&state, generation)?;
        let photo = if session
            .connection
            .as_ref()
            .map(|connection| matches!(connection.conn_type, ConnectionType::USB))
            .unwrap_or(false)
        {
            session
                .usb_catalog
                .iter()
                .find(|record| record.photo.handle == handle)
                .map(|record| record.photo.clone())
        } else {
            session
                .wifi_catalog
                .iter()
                .find(|photo| photo.handle == handle)
                .cloned()
        };
        match photo {
            Some(photo) => session
                .source_cache
                .lookup(handle, &photo.filename, photo.size)
                .map_err(|error| format!("Unable to inspect camera source cache: {error}"))?
                .map(|path| (generation, photo, path)),
            None => None,
        }
    };
    if let Some((generation, photo, source)) = cached_source {
        let destination = Path::new(&dest_path);
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::Builder::new()
            .prefix(".nikon-photo-studio-")
            .suffix(".part")
            .tempfile_in(parent)
            .map_err(|error| format!("Unable to create download staging file: {error}"))?;
        emit_download_progress(&app, handle, &photo.filename, 0, photo.size);
        let copied = std::io::copy(
            &mut File::open(&source)
                .map_err(|error| format!("Unable to open cached camera original: {error}"))?,
            temporary.as_file_mut(),
        )
        .map_err(|error| format!("Unable to copy cached camera original: {error}"))?;
        temporary
            .as_file_mut()
            .flush()
            .map_err(|error| format!("Unable to flush cached camera original: {error}"))?;
        ensure_active_session(&state, generation)?;
        if copied != photo.size {
            return Err(format!(
                "Cached camera original length mismatch for {}: {copied} / {}",
                photo.filename, photo.size
            ));
        }
        temporary.persist(destination).map_err(|error| {
            format!("Unable to publish cached camera original: {}", error.error)
        })?;
        emit_download_progress(&app, handle, &photo.filename, copied, copied);
        return Ok(dest_path);
    }

    let (generation, usb_record, wifi_client, cached_photo) = {
        let session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        let generation = session.generation;
        ensure_active_session(&state, generation)?;
        let is_usb = session
            .connection
            .as_ref()
            .map(|connection| matches!(connection.conn_type, ConnectionType::USB))
            .ok_or("Not connected")?;
        if is_usb {
            let record = session
                .usb_catalog
                .iter()
                .find(|record| record.photo.handle == handle)
                .cloned()
                .ok_or_else(|| format!("USB photo handle {handle} not found"))?;
            (generation, Some(record), None, None)
        } else {
            (
                generation,
                None,
                Some(
                    session
                        .wifi_client
                        .as_ref()
                        .cloned()
                        .ok_or("Wi-Fi session is not active; reconnect the camera")?,
                ),
                session
                    .wifi_catalog
                    .iter()
                    .find(|photo| photo.handle == handle)
                    .cloned(),
            )
        }
    };

    if let Some(shared_client) = wifi_client {
        let mut client = shared_client
            .lock()
            .map_err(|_| "Wi-Fi PTP/IP client lock is unavailable")?;
        ensure_active_session(&state, generation)?;
        let photo = match cached_photo {
            Some(photo) => photo,
            None => client
                .get_object_info(handle)
                .map_err(|error| error.to_string())?,
        };
        let destination = Path::new(&dest_path);
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::Builder::new()
            .prefix(".nikon-photo-studio-")
            .suffix(".part")
            .tempfile_in(parent)
            .map_err(|error| format!("Unable to create download staging file: {error}"))?;

        emit_download_progress(&app, handle, &photo.filename, 0, photo.size);
        let mut last_emitted = 0u64;
        let received = client
            .get_object_to_writer(
                handle,
                temporary.as_file_mut(),
                |bytes_received, total_bytes| {
                    const EVENT_STEP: u64 = 512 * 1024;
                    if bytes_received == 0
                        || bytes_received == total_bytes
                        || bytes_received.saturating_sub(last_emitted) >= EVENT_STEP
                    {
                        emit_download_progress(
                            &app,
                            handle,
                            &photo.filename,
                            bytes_received,
                            total_bytes,
                        );
                        last_emitted = bytes_received;
                    }
                },
            )
            .map_err(|error| error.to_string())?;
        temporary
            .as_file_mut()
            .flush()
            .map_err(|error| format!("Unable to flush download: {error}"))?;
        if photo.size > 0 && received != photo.size {
            return Err(format!(
                "Downloaded byte count mismatch for {}: {received} / {}",
                photo.filename, photo.size
            ));
        }
        ensure_active_session(&state, generation)?;
        temporary
            .persist(destination)
            .map_err(|error| format!("Unable to publish completed download: {}", error.error))?;
        emit_download_progress(&app, handle, &photo.filename, received, received);
        return Ok(dest_path);
    }

    if let Some(record) = usb_record {
        let _io = state
            .usb_io
            .lock()
            .map_err(|_| "USB I/O lock is unavailable")?;
        ensure_active_session(&state, generation)?;
        emit_download_progress(&app, handle, &record.photo.filename, 0, record.photo.size);
        super::usb::UsbCamera::download(&record, &dest_path).map_err(|e| e.to_string())?;
        emit_download_progress(
            &app,
            handle,
            &record.photo.filename,
            record.photo.size,
            record.photo.size,
        );
    }

    Ok(dest_path)
}

#[tauri::command]
pub async fn get_thumbnail(handle: u32, state: State<'_, CameraState>) -> Result<Vec<u8>, String> {
    let (generation, shared_client) = {
        let session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        let generation = session.generation;
        ensure_active_session(&state, generation)?;
        let is_usb = session
            .connection
            .as_ref()
            .map(|connection| matches!(connection.conn_type, ConnectionType::USB))
            .ok_or("Not connected")?;
        if is_usb {
            return Err("Windows MTP does not expose camera thumbnails directly; download the file to preview it".into());
        }
        (
            generation,
            session
                .wifi_client
                .as_ref()
                .cloned()
                .ok_or("Wi-Fi session is not active; reconnect the camera")?,
        )
    };
    let mut client = shared_client
        .lock()
        .map_err(|_| "Wi-Fi PTP/IP client lock is unavailable")?;
    ensure_active_session(&state, generation)?;
    let thumbnail = client.get_thumbnail(handle).map_err(|e| e.to_string())?;
    ensure_active_session(&state, generation)?;
    Ok(thumbnail)
}

#[tauri::command]
pub async fn get_camera_thumbnails(
    handles: Vec<u32>,
    size: Option<u32>,
    state: State<'_, CameraState>,
) -> Result<Vec<CameraThumbnail>, String> {
    let (generation, usb_records, wifi_client, mut output, pending) = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        let generation = session.generation;
        ensure_active_session(&state, generation)?;
        let is_usb = session
            .connection
            .as_ref()
            .map(|connection| matches!(connection.conn_type, ConnectionType::USB))
            .ok_or("Not connected")?;
        if is_usb {
            let records: Vec<_> = handles
                .iter()
                .filter_map(|handle| {
                    session
                        .usb_catalog
                        .iter()
                        .find(|record| record.photo.handle == *handle)
                        .cloned()
                })
                .collect();
            (generation, Some(records), None, Vec::new(), Vec::new())
        } else {
            let client = session
                .wifi_client
                .as_ref()
                .cloned()
                .ok_or("Wi-Fi session is not active; reconnect the camera")?;
            let mut output = Vec::with_capacity(handles.len());
            let mut pending = Vec::with_capacity(handles.len());
            let mut seen = std::collections::HashSet::with_capacity(handles.len());
            for handle in handles {
                if !seen.insert(handle) {
                    continue;
                }
                if let Some(url) = session.thumbnail_cache.get_cloned(handle) {
                    output.push(CameraThumbnail { handle, url });
                    continue;
                }
                pending.push(handle);
            }
            (generation, None, Some(client), output, pending)
        }
    };

    if let Some(records) = usb_records {
        let _io = state
            .usb_io
            .lock()
            .map_err(|_| "USB I/O lock is unavailable")?;
        ensure_active_session(&state, generation)?;
        return super::usb::UsbCamera::thumbnails(&records, size.unwrap_or(320))
            .map(|rows| {
                rows.into_iter()
                    .map(|row| CameraThumbnail {
                        handle: row.handle,
                        url: format!("data:image/jpeg;base64,{}", row.data),
                    })
                    .collect()
            })
            .map_err(|error| error.to_string());
    }

    let shared_client = wifi_client.expect("Wi-Fi branch publishes a client");
    let mut client = shared_client
        .lock()
        .map_err(|_| "Wi-Fi PTP/IP client lock is unavailable")?;
    let mut fetched = Vec::with_capacity(pending.len());
    // Nikon accepts one PTP transaction at a time. Keep the camera operations
    // serial on the client lock, but leave CameraSession free for cancellation.
    for handle in pending {
        ensure_active_session(&state, generation)?;
        if let Ok(bytes) = client.get_thumbnail(handle) {
            fetched.push((handle, crate::library::commands::data_url(&bytes)));
        }
    }
    drop(client);
    ensure_active_session(&state, generation)?;
    let mut session = state
        .session
        .lock()
        .map_err(|_| "Camera session lock is unavailable")?;
    if session.generation != generation {
        return Err(SUPERSEDED.to_string());
    }
    for (handle, url) in fetched {
        session.thumbnail_cache.insert(handle, url.clone());
        output.push(CameraThumbnail { handle, url });
    }
    Ok(output)
}

#[tauri::command]
pub async fn prepare_camera_photo(
    handle: u32,
    app: AppHandle,
    state: State<'_, CameraState>,
) -> Result<PreparedCameraPhoto, String> {
    let (photo, path) = prepare_camera_source(handle, &app, &state)?;
    Ok(PreparedCameraPhoto {
        path: path
            .to_str()
            .ok_or("camera preview cache path is not UTF-8")?
            .to_string(),
        handle: photo.handle,
        filename: photo.filename,
        size: photo.size,
        format: photo.format,
        width: photo.width,
        height: photo.height,
    })
}

/// Compatibility endpoint retained for older frontend builds.  It now shares
/// the stable camera-source cache and accepts an adaptive render size, so a
/// 2048px request followed by 6144/8192px zoom upgrades transfers the original
/// only once.
#[tauri::command]
pub async fn get_camera_preview(
    handle: u32,
    max_size: Option<u32>,
    app: AppHandle,
    state: State<'_, CameraState>,
) -> Result<String, String> {
    let (_, filename) = prepare_camera_source(handle, &app, &state)?;
    crate::library::commands::make_full_preview(
        filename
            .to_str()
            .ok_or("camera preview cache path is not UTF-8")?
            .to_string(),
        Some(max_size.unwrap_or(6144).clamp(1024, 8192)),
    )
    .await
}

#[tauri::command]
pub async fn delete_camera_photos(
    handles: Vec<u32>,
    state: State<'_, CameraState>,
) -> Result<DeleteCameraResult, String> {
    if handles
        .iter()
        .any(|handle| *handle == 0 || *handle == u32::MAX)
    {
        return Err("拒绝危险或无效的照片句柄；请重新扫描相机".to_string());
    }
    let mut result = DeleteCameraResult {
        deleted: Vec::new(),
        protected: Vec::new(),
        failed: Vec::new(),
    };
    let (generation, usb_records, wifi_client) = {
        let session = state
            .session
            .lock()
            .map_err(|_| "Camera session lock is unavailable")?;
        let generation = session.generation;
        ensure_active_session(&state, generation)?;
        let is_usb = session
            .connection
            .as_ref()
            .map(|connection| matches!(connection.conn_type, ConnectionType::USB))
            .ok_or("Not connected")?;
        if is_usb {
            let records: Vec<_> = handles
                .iter()
                .filter_map(|handle| {
                    match session
                        .usb_catalog
                        .iter()
                        .find(|record| record.photo.handle == *handle)
                        .cloned()
                    {
                        Some(record) => Some(record),
                        None => {
                            result.failed.push(format!(
                                "句柄 {handle}：照片不在当前 USB 目录中，请重新扫描相机"
                            ));
                            None
                        }
                    }
                })
                .collect();
            (generation, Some(records), None)
        } else {
            (
                generation,
                None,
                Some(
                    session
                        .wifi_client
                        .as_ref()
                        .cloned()
                        .ok_or("Wi-Fi session is not active; reconnect the camera")?,
                ),
            )
        }
    };

    if let Some(shared_client) = wifi_client {
        let mut client = shared_client
            .lock()
            .map_err(|_| "Wi-Fi PTP/IP client lock is unavailable")?;
        for handle in handles {
            if !session_is_current(&state, generation) {
                result
                    .failed
                    .push("相机会话已切换，剩余照片未删除".to_string());
                break;
            }
            let photo = match client.get_object_info(handle) {
                Ok(photo) => photo,
                Err(error) => {
                    result.failed.push(format!("句柄 {handle}：{error}"));
                    continue;
                }
            };
            if photo.protected || !photo.can_delete {
                result.protected.push(photo.filename);
                continue;
            }
            match client.delete_object(handle) {
                Ok(()) => result.deleted.push(handle),
                Err(error)
                    if error.to_string().contains("受保护")
                        || error.to_string().contains("0x200D") =>
                {
                    result.protected.push(photo.filename);
                }
                Err(error) => result.failed.push(format!("{}：{}", photo.filename, error)),
            }
        }
        drop(client);
        ensure_active_session(&state, generation)?;
        if !result.deleted.is_empty() {
            let deleted: std::collections::HashSet<_> = result.deleted.iter().copied().collect();
            let mut session = state
                .session
                .lock()
                .map_err(|_| "Camera session lock is unavailable")?;
            if session.generation != generation {
                return Err(SUPERSEDED.to_string());
            }
            session
                .wifi_catalog
                .retain(|photo| !deleted.contains(&photo.handle));
            for handle in &result.deleted {
                session.thumbnail_cache.remove(*handle);
            }
        }
        return Ok(result);
    }

    if let Some(records) = usb_records {
        let _io = state
            .usb_io
            .lock()
            .map_err(|_| "USB I/O lock is unavailable")?;
        ensure_active_session(&state, generation)?;
        for record in records {
            if !session_is_current(&state, generation) {
                result
                    .failed
                    .push("相机会话已切换，剩余照片未删除".to_string());
                break;
            }
            if record.photo.protected || !record.photo.can_delete {
                result.protected.push(record.photo.filename.clone());
                continue;
            }
            match super::usb::UsbCamera::delete(&record) {
                Ok(()) => result.deleted.push(record.photo.handle),
                Err(error)
                    if error.to_string().contains("PROTECTED")
                        || error.to_string().contains("protected") =>
                {
                    result.protected.push(record.photo.filename.clone());
                }
                Err(error) => result
                    .failed
                    .push(format!("{}：{}", record.photo.filename, error)),
            }
        }
        drop(_io);
        if !result.deleted.is_empty() {
            let deleted: std::collections::HashSet<_> = result.deleted.iter().copied().collect();
            let mut session = state
                .session
                .lock()
                .map_err(|_| "Camera session lock is unavailable")?;
            if session_is_current(&state, generation) && session.generation == generation {
                session
                    .usb_catalog
                    .retain(|record| !deleted.contains(&record.photo.handle));
            }
        }
        return Ok(result);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        begin_connection, connection_progress_payload, ensure_active_session, parse_wifi_ssid,
    };
    use crate::camera::CameraState;

    #[test]
    fn parses_ssid_without_confusing_bssid() {
        let output = "    Name                   : Wi-Fi\n    SSID                   : NIKON_Z_30_59598\n    BSSID                  : 01:02:03:04:05:06\n";
        assert_eq!(parse_wifi_ssid(output).as_deref(), Some("NIKON_Z_30_59598"));
    }

    #[test]
    fn missing_wifi_has_no_ssid() {
        assert_eq!(parse_wifi_ssid("There is no wireless interface"), None);
    }

    #[test]
    fn usb_progress_is_a_single_attempt_camel_case_payload() {
        let progress =
            connection_progress_payload(1, 1, "usb-storage", "正在等待存储卡并读取照片目录");
        let json = serde_json::to_value(progress).expect("serialize connection progress");

        assert_eq!(json["attempt"], 1);
        assert_eq!(json["totalAttempts"], 1);
        assert_eq!(json["phase"], "usb-storage");
        assert!(json["message"]
            .as_str()
            .expect("message")
            .contains("存储卡"));
    }

    #[test]
    fn newer_connection_generation_invalidates_older_owner() {
        let state = CameraState::default();
        let first = begin_connection(&state).expect("first connection owner");
        let second = begin_connection(&state).expect("second connection owner");

        assert!(ensure_active_session(&state, first).is_err());
        assert!(ensure_active_session(&state, second).is_ok());
        let session = state.session.lock().expect("camera session lock");
        assert_eq!(session.generation, second);
        assert!(session.connection.is_none());
        assert!(session.wifi_client.is_none());
        assert!(session.usb_catalog.is_empty());
    }
}
