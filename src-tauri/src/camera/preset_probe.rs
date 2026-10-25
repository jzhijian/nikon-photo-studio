//! Storage discovery is deliberately not reported as proof of PTP setting support.
use serde_json::{json, Value};

fn report(devices: Vec<(String, String)>) -> Value {
    json!({
        "schemaVersion": 1,
        "backend": "windows-wpd-storage-discovery",
        "devices": devices.into_iter().map(|(name, id)| json!({"name":name,"id":id})).collect::<Vec<_>>(),
        "firmware": null,
        "sdkLoaded": false,
        "selectPictureControl": "unknown",
        "writeCustomPictureControl": "unknown",
        "persistentAfterDisconnect": "unknown",
        "canWritePreset": false,
        "reason": "本次只检测 USB 存储设备；尚未接入 Nikon SDK，不能证明优化校准可写。"
    })
}

#[tauri::command]
pub async fn probe_preset_usb() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        super::usb::UsbCamera::scan_devices().map(report).map_err(|e| e.to_string())
    }).await.map_err(|_| "USB 检测任务失败".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovering_z30_does_not_grant_write_capability() {
        for devices in [vec![], vec![("NIKON Z 30".into(), "test-device".into())]] {
            let value = report(devices);
            assert_eq!(value["canWritePreset"], false);
            assert_eq!(value["writeCustomPictureControl"], "unknown");
            assert!(value["firmware"].is_null());
        }
    }
}
