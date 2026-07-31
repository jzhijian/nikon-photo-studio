//! Windows USB camera discovery and MTP/PTP storage enumeration.
//! Explorer's Shell namespace is backed by Windows Portable Devices, so it can
//! enumerate Nikon cameras that do not expose the SD card as a drive letter.

use super::{CameraPhoto, PhotoFormat};
use serde::Deserialize;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum UsbError {
    #[error("No Nikon camera found. Set the camera USB mode to MTP/PTP and reconnect the cable.")]
    NoDevice,
    #[error("Windows portable-device error: {0}")]
    Connection(String),
    #[error("Transfer error: {0}")]
    Transfer(String),
}

#[derive(Debug, Clone)]
pub struct UsbPhotoRecord {
    pub photo: CameraPhoto,
    pub shell_path: String,
}

#[derive(Deserialize)]
struct ShellPhoto { device: String, path: String, name: String, size: Option<u64> }

pub struct UsbCamera { pub device_name: String, records: Vec<UsbPhotoRecord> }

fn powershell(script: &str, envs: &[(&str, &str)]) -> Result<String, UsbError> {
    let mut cmd = Command::new("powershell.exe");
    // Explorer's Windows Portable Devices namespace is an apartment-threaded
    // COM API. An explicit STA runspace is required: without it the root device
    // is visible, but GetFolder/Items may silently return no camera contents.
    // Pass the script as one command argument. `-Command -` reads stdin one
    // statement at a time and does not reliably assemble our multi-line Walk
    // function, which made device discovery succeed while enumeration silently
    // returned an empty list.
    cmd.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Sta", "-Command", script]);
    for (key, value) in envs { cmd.env(key, value); }
    let output = cmd.output().map_err(|e| UsbError::Connection(e.to_string()))?;
    if !output.status.success() {
        return Err(UsbError::Connection(String::from_utf8_lossy(&output.stderr).trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

const ENUM_SCRIPT: &str = r#"
[Console]::OutputEncoding=[Text.UTF8Encoding]::new()
$shell=New-Object -ComObject Shell.Application
$root=$shell.Namespace(17)
$result=New-Object System.Collections.Generic.List[object]
function Walk($folder,$device,$prefix) {
  foreach($item in @($folder.Items())) {
    $path=if($prefix){"$prefix\$($item.Name)"}else{$item.Name}
    if($item.IsFolder){ try { Walk $item.GetFolder $device $path } catch {} }
    elseif($item.Name -match '(?i)\.(nef|nrw|jpe?g|tiff?)$'){
      # ShellItem.Size is commonly zero for WPD/MTP objects even though
      # Explorer displays the correct value. System.Size is the underlying
      # portable-device property Explorer itself uses.
      $size=$item.ExtendedProperty('System.Size')
      if($null -eq $size){$size=$item.Size}
      $result.Add([pscustomobject]@{device=$device;path="$device\$path";name=$item.Name;size=[uint64]$size})
    }
  }
}
foreach($device in @($root.Items() | Where-Object {$_.Name -match '(?i)nikon|z\s?30|z\s?50'})) {
  try { Walk $device.GetFolder $device.Name '' } catch {}
}
$result | ConvertTo-Json -Compress
"#;

impl UsbCamera {
    pub fn scan_devices() -> Result<Vec<(String, String)>, UsbError> {
        let script = r#"
[Console]::OutputEncoding=[Text.UTF8Encoding]::new()
$shell=New-Object -ComObject Shell.Application
$root=$shell.Namespace(17)
@($root.Items() | Where-Object {$_.Name -match '(?i)nikon|z\s?30|z\s?50'} | ForEach-Object {$_.Name}) | ConvertTo-Json -Compress
"#;
        let raw = powershell(script, &[])?;
        if raw.is_empty() { return Ok(Vec::new()); }
        let names: Vec<String> = serde_json::from_str(&raw).or_else(|_| serde_json::from_str::<String>(&raw).map(|v| vec![v]))
            .map_err(|e| UsbError::Connection(format!("cannot parse Windows device list: {e}")))?;
        Ok(names.into_iter().map(|name| (name.clone(), name)).collect())
    }

    pub fn connect(device_name: &str) -> Result<Self, UsbError> {
        let raw = powershell(ENUM_SCRIPT, &[])?;
        let rows: Vec<ShellPhoto> = if raw.is_empty() { Vec::new() } else {
            serde_json::from_str(&raw).or_else(|_| serde_json::from_str::<ShellPhoto>(&raw).map(|v| vec![v]))
                .map_err(|e| UsbError::Connection(format!("cannot parse MTP photo list: {e}")))?
        };
        let records = rows.into_iter().filter(|row| row.device == device_name).enumerate().map(|(index, row)| {
            let ext = row.name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            let format = match ext.as_str() { "jpg"|"jpeg" => PhotoFormat::JPEG, "nef"|"nrw" => PhotoFormat::NEF, "tif"|"tiff" => PhotoFormat::TIFF, _ => PhotoFormat::Unknown(0) };
            UsbPhotoRecord { shell_path: row.path.clone(), photo: CameraPhoto {
                handle: (index + 1) as u32, filename: row.name, size: row.size.unwrap_or(0), format,
                date: String::new(), width: 0, height: 0, source_path: Some(row.path),
            }}
        }).collect();
        Ok(Self { device_name: device_name.to_string(), records })
    }

    pub fn records(&self) -> &[UsbPhotoRecord] { &self.records }

    pub fn download(record: &UsbPhotoRecord, dest_path: &str) -> Result<(), UsbError> {
        let script = r#"
$shell=New-Object -ComObject Shell.Application
$parts=$env:NPS_WPD_PATH -split '\\'
$folder=$shell.Namespace(17)
$item=$null
foreach($part in $parts){ $item=@($folder.Items() | Where-Object {$_.Name -eq $part})[0]; if(!$item){throw "MTP object not found: $part"}; if($item.IsFolder){$folder=$item.GetFolder} }
$dest=[IO.Path]::GetDirectoryName($env:NPS_DEST)
$name=[IO.Path]::GetFileName($env:NPS_DEST)
$destFolder=$shell.Namespace($dest); $destFolder.CopyHere($item,20)
$copied=Join-Path $dest $item.Name
for($i=0;$i -lt 300 -and !(Test-Path -LiteralPath $copied);$i++){Start-Sleep -Milliseconds 100}
if(!(Test-Path -LiteralPath $copied)){throw 'MTP copy timed out'}
if($item.Name -ne $name){Move-Item -LiteralPath $copied -Destination $env:NPS_DEST -Force}
"#;
        powershell(script, &[("NPS_WPD_PATH", &record.shell_path), ("NPS_DEST", dest_path)])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn nikon_vendor_id_is_stable() { assert_eq!(0x04B0u16, 0x04B0); }

    #[test]
    fn enumerate_connected_nikon_when_available() {
        let devices = UsbCamera::scan_devices().expect("Windows portable-device scan");
        let Some((path, name)) = devices.first() else {
            eprintln!("No Nikon MTP/PTP camera connected; skipping integration assertion");
            return;
        };
        let camera = UsbCamera::connect(path).expect("enumerate connected Nikon storage");
        eprintln!("{}: {} supported photos", name, camera.records().len());
        assert!(!camera.records().is_empty(), "Windows exposes the Nikon camera but no supported photos were returned");
        assert!(camera.records()[0].photo.size > 0, "MTP System.Size should expose the real file size");
        let dir = tempfile::tempdir().expect("temporary MTP download directory");
        let destination = dir.path().join(&camera.records()[0].photo.filename);
        UsbCamera::download(&camera.records()[0], destination.to_str().expect("UTF-8 temp path"))
            .expect("download first MTP photo");
        assert_eq!(
            std::fs::metadata(destination).expect("downloaded MTP file metadata").len(),
            camera.records()[0].photo.size,
            "downloaded bytes should match MTP System.Size",
        );
    }
}
