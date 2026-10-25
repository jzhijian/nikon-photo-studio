//! Runs the application-owned read-only Nikon SDK helper out of process.
//! The proprietary SDK is installed separately under Nikon's agreement.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Capability { id: u32, can_get: bool, can_set: bool }

#[derive(Deserialize, Serialize)]
struct Device { name: String, available: bool, capabilities: Vec<Capability> }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProbeOutput { schema_version: u32, sdk_loaded: bool, devices: Vec<Device> }

fn parse_probe_output(bytes: &[u8]) -> Result<Value, String> {
    // Official SDK diagnostics may precede the helper's final JSON line.
    let line = bytes.split(|b| *b == b'\n').rev().find(|line| !line.iter().all(u8::is_ascii_whitespace))
        .ok_or("SDK 检测程序未返回结果")?;
    let value: Value = serde_json::from_slice(line).map_err(|_| "SDK 检测结果格式错误")?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(format!("Nikon SDK：{error}"));
    }
    let report: ProbeOutput = serde_json::from_value(value).map_err(|_| "SDK 检测结果字段不完整")?;
    if report.schema_version != 1 || !report.sdk_loaded || report.devices.len() > 100 {
        return Err("SDK 检测结果尚未通过验证".into());
    }
    let reason = if report.devices.is_empty() {
        "Nikon SDK 已成功加载，未发现相机。请开机并连接 USB 数据线，关闭占用相机的尼康软件后重试。"
    } else {
        "已读取 Nikon SDK 设备能力；支持设置的标记仍需通过预设写入、读回与实拍验证。"
    };
    Ok(json!({"schemaVersion":1, "backend":"nikon-remote-sdk", "sdkLoaded":true,
        "devices":report.devices, "canWritePreset":false, "reason":reason}))
}

#[tauri::command]
pub async fn probe_nikon_sdk(sdk_directory: String) -> Result<Value, String> {
    let sdk = PathBuf::from(sdk_directory).canonicalize().map_err(|_| "找不到 SDK 文件夹")?;
    if !sdk.join("ControlServiceLayer.dll").is_file() {
        return Err("请选择 SDK 中包含 ControlServiceLayer.dll 的 Module/Win/BinaryFile 文件夹".into());
    }
    let helper = std::env::current_exe().map_err(|_| "无法定位检测程序")?
        .with_file_name("nikon-sdk-probe.exe");
    if !helper.is_file() { return Err("缺少 nikon-sdk-probe.exe，请先构建 SDK 检测程序并放在应用旁".into()); }
    let mut command = tokio::process::Command::new(helper);
    command.arg(sdk).kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let output = tokio::time::timeout(Duration::from_secs(30), command.output()).await
        .map_err(|_| "SDK 检测超时，检测进程已停止；请确认相机未被其他软件占用")?
        .map_err(|e| format!("无法启动 SDK 检测程序：{e}"))?;
    let report = parse_probe_output(&output.stdout)?;
    if !output.status.success() { return Err("SDK 检测程序异常结束".into()); }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertised_set_capability_does_not_claim_a_verified_write() {
        let raw = br#"{"schemaVersion":1,"sdkLoaded":true,"canWritePreset":true,"devices":[{"name":"NIKON Z 30","available":true,"capabilities":[{"id":33326,"canGet":true,"canSet":true}]}]}"#;
        let report = parse_probe_output(raw).unwrap();
        assert_eq!(report["sdkLoaded"], true);
        assert_eq!(report["canWritePreset"], false);
        assert_eq!(report["devices"][0]["capabilities"][0]["canSet"], true);
    }

    #[test]
    fn errors_and_malformed_bridge_output_never_become_empty_device_success() {
        for raw in [b"not json".as_slice(), br#"{"sdkLoaded":true}"#, br#"{"error":"profiles missing","canWritePreset":false}"#] {
            assert!(parse_probe_output(raw).is_err());
        }
    }
}
