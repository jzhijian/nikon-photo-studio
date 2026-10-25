//! Windows USB camera discovery and MTP/PTP storage enumeration.
//! Explorer's Shell namespace is backed by Windows Portable Devices, so it can
//! enumerate Nikon cameras that do not expose the SD card as a drive letter.

use super::{CameraPhoto, PhotoFormat};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
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
struct ShellPhoto {
    device: String,
    path: String,
    name: String,
    size: Option<u64>,
    #[serde(default)]
    protected: bool,
    #[serde(default, rename = "canDelete")]
    can_delete: bool,
}

#[derive(Debug, Serialize)]
pub struct UsbThumbnail {
    pub handle: u32,
    pub data: String,
}

#[derive(Deserialize)]
struct ShellThumbnail {
    handle: u32,
    #[serde(default)]
    data: String,
    #[serde(default)]
    error: Option<String>,
}

pub struct UsbCamera {
    pub device_name: String,
    records: Vec<UsbPhotoRecord>,
}

fn powershell(script: &str, envs: &[(&str, &str)]) -> Result<String, UsbError> {
    let mut cmd = Command::new("powershell.exe");
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
                                     // Explorer's Windows Portable Devices namespace is an apartment-threaded
                                     // COM API. An explicit STA runspace is required: without it the root device
                                     // is visible, but GetFolder/Items may silently return no camera contents.
                                     // Pass the script as one command argument. `-Command -` reads stdin one
                                     // statement at a time and does not reliably assemble our multi-line Walk
                                     // function, which made device discovery succeed while enumeration silently
                                     // returned an empty list.
    cmd.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Sta",
        "-Command",
        script,
    ]);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let output = cmd
        .output()
        .map_err(|e| UsbError::Connection(e.to_string()))?;
    if !output.status.success() {
        return Err(UsbError::Connection(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn is_jpeg_thumbnail_payload(data: &str) -> bool {
    // The PowerShell bridge always re-encodes a successful thumbnail as JPEG.
    // Reject malformed/diagnostic output before it can become a data URL.
    data.starts_with("/9j/")
}

/// Windows Shell/WPD providers are inconsistent about System.FileAttributes:
/// normal files may be numeric, an enum name, or compact flags such as `N`,
/// while protected Nikon objects can arrive as `R`/`RN`. Keep this exact
/// helper shared by enumeration and the final pre-delete protection check.
const READ_ONLY_ATTRIBUTE_FUNCTION: &str = r#"
function TestNpsReadOnlyAttribute($value) {
  if($null -eq $value){return $false}
  $text=("$value").Trim()
  [uint64]$number=0
  if([uint64]::TryParse($text,[ref]$number)){return (($number -band 1) -ne 0)}
  if($text -match '(?i)(^|[,\s])ReadOnly($|[,\s])'){return $true}
  # WPD compact flags use letters such as N (normal), A (archive), and R
  # (read-only). Restrict this branch to flag letters so words like Normal or
  # Archive are not misclassified merely because they contain the letter r.
  if($text -match '^[RrNnAaHhSsDd]+$'){return ($text -match '[Rr]')}
  return $false
}
"#;

const ENUM_SCRIPT: &str = r#"
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new()
$shell=New-Object -ComObject Shell.Application
$root=$shell.Namespace(17)
$result=New-Object System.Collections.Generic.List[object]
$deleteCapability=@{}
$walkErrors=New-Object System.Collections.Generic.List[string]
function FindDeleteVerb($item) {
  foreach($verb in @($item.Verbs())) {
    $name=("$($verb.Name)" -replace '&','').Trim()
    if($name -match '(?i)^(delete|remove)(\s|$)|删除|刪除'){return $verb}
  }
  return $null
}
function ReadItems($folder,[string]$context,[bool]$waitForContent) {
  $lastError=$null
  # A portable device can appear in This PC slightly before its storage tree
  # is ready. Explorer retries this internally; do the same instead of
  # publishing a false, permanent zero-photo snapshot.
  for($attempt=0;$attempt -lt 16;$attempt++) {
    try {
      $items=@($folder.Items())
      if(!$waitForContent -or $items.Count -gt 0){return $items}
    } catch {
      $lastError=$_.Exception.Message
    }
    Start-Sleep -Milliseconds 250
  }
  if($null -ne $lastError){throw "Unable to enumerate ${context}: $lastError"}
  return @()
}
function Walk($folder,$device,$prefix,[int]$depth) {
  if($depth -gt 16){$walkErrors.Add("MTP folder nesting exceeds 16 levels at $prefix");return}
  $items=@()
  try {$items=@(ReadItems $folder "$device\$prefix" $false)}
  catch {$walkErrors.Add($_.Exception.Message);return}
  foreach($item in $items) {
    $path=if($prefix){"$prefix\$($item.Name)"}else{$item.Name}
    $isFolder=$false
    try {$isFolder=[bool]$item.IsFolder} catch {}
    if($isFolder){
      try { Walk $item.GetFolder $device $path ($depth+1) }
      catch {$walkErrors.Add("Unable to open $device\${path}: $($_.Exception.Message)")}
    }
    elseif(("$($item.Name)") -match '(?i)\.(nef|nrw|jpe?g|tiff?)$'){
      # ShellItem.Size is commonly zero for WPD/MTP objects even though
      # Explorer displays the correct value. System.Size is the underlying
      # portable-device property Explorer itself uses. Some Nikon/Windows WPD
      # combinations transiently fail this one metadata property. A failure
      # must not abort the entire DCIM branch; preserve the photo with size 0
      # and let a later scan refresh it.
      [uint64]$size=0
      try {
        $systemSize=$item.ExtendedProperty('System.Size')
        if($null -ne $systemSize -and "$systemSize" -ne ''){$size=[uint64]$systemSize}
      } catch {}
      if($size -eq 0){
        try {
          $itemSize=$item.Size
          if($null -ne $itemSize -and "$itemSize" -ne ''){$size=[uint64]$itemSize}
        } catch {}
      }
      # Nikon's in-camera Protect flag is exposed by the Windows portable
      # device provider as the read-only file attribute. Do not infer delete
      # support merely from a missing protection value: query the Shell/WPD
      # delete capability once per device and combine both signals.
      $attributes=$null
      try {$attributes=$item.ExtendedProperty('System.FileAttributes')} catch {}
      $isProtected=[bool](TestNpsReadOnlyAttribute $attributes)
      $canDelete=$false
      if(!$isProtected) {
        if(!$deleteCapability.ContainsKey($device)) {
          try {$deleteCapability[$device]=($null -ne (FindDeleteVerb $item))}
          catch {$deleteCapability[$device]=$false}
        }
        $canDelete=[bool]$deleteCapability[$device]
      }
      $result.Add([pscustomobject]@{device=$device;path="$device\$path";name=$item.Name;size=[uint64]$size;protected=$isProtected;canDelete=$canDelete})
    }
  }
}
$deviceName=$env:NPS_WPD_DEVICE
$device=$null
for($attempt=0;$attempt -lt 16 -and $null -eq $device;$attempt++) {
  $device=@($root.Items() | Where-Object {$_.Name -eq $deviceName})[0]
  if($null -eq $device){Start-Sleep -Milliseconds 250}
}
if($null -eq $device){throw "MTP device disappeared before storage enumeration: $deviceName"}
$deviceFolder=$device.GetFolder
$topLevel=@(ReadItems $deviceFolder $device.Name $true)
if($topLevel.Count -eq 0){throw "MTP device is visible but its storage tree is not ready: $deviceName"}
foreach($item in $topLevel) {
  $path=$item.Name
  $isFolder=$false
  try {$isFolder=[bool]$item.IsFolder} catch {}
  if($isFolder){
    try {Walk $item.GetFolder $device.Name $path 1}
    catch {$walkErrors.Add("Unable to open $($device.Name)\${path}: $($_.Exception.Message)")}
  }
}
if($result.Count -eq 0 -and $walkErrors.Count -gt 0){throw $walkErrors[0]}
$result | ConvertTo-Json -Compress
"#;

const THUMBNAIL_SCRIPT: &str = r#"
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new()
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
[StructLayout(LayoutKind.Sequential)] public struct NpsSize { public int cx; public int cy; }
[ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("bcc18b79-ba16-442f-80c4-8a59c30c463b")]
public interface INpsShellItemImageFactory { [PreserveSig] int GetImage(NpsSize size, uint flags, out IntPtr bitmap); }
public static class NpsShellThumbnail {
  [DllImport("shell32.dll")] static extern int SHGetIDListFromObject(IntPtr value, out IntPtr pidl);
  [DllImport("shell32.dll")] static extern int SHCreateItemFromIDList(IntPtr pidl, ref Guid iid, [MarshalAs(UnmanagedType.Interface)] out object value);
  [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr value);
  public static string Read(object shellObject, int size) {
    IntPtr unknown=IntPtr.Zero, pidl=IntPtr.Zero, bitmap=IntPtr.Zero;
    object factory=null;
    try {
      unknown=Marshal.GetIUnknownForObject(shellObject);
      int result=SHGetIDListFromObject(unknown,out pidl); if(result!=0) Marshal.ThrowExceptionForHR(result);
      Guid iid=new Guid("bcc18b79-ba16-442f-80c4-8a59c30c463b");
      result=SHCreateItemFromIDList(pidl,ref iid,out factory); if(result!=0) Marshal.ThrowExceptionForHR(result);
      // Every request includes SIIGBF_THUMBNAILONLY (0x8). Omitting that bit
      // permits Windows to return the generic NEF/camera icon, which must never
      // be presented as photo content. BIGGERSIZEOK lets the camera/provider
      // return its largest embedded preview instead of first shrinking it.
      foreach(uint flags in new uint[]{0x9,0x8}) {
        result=((INpsShellItemImageFactory)factory).GetImage(new NpsSize{cx=size,cy=size},flags,out bitmap);
        if(result==0 && bitmap!=IntPtr.Zero) break;
        if(bitmap!=IntPtr.Zero){DeleteObject(bitmap);bitmap=IntPtr.Zero;}
      }
      if(result!=0) Marshal.ThrowExceptionForHR(result);
      if(bitmap==IntPtr.Zero) throw new COMException("The camera did not expose a thumbnail resource",unchecked((int)0x80004005));
      using(var image=Image.FromHbitmap(bitmap)) using(var stream=new MemoryStream()) {
        var codec=Array.Find(ImageCodecInfo.GetImageEncoders(),c=>c.FormatID==ImageFormat.Jpeg.Guid);
        using(var parameters=new EncoderParameters(1)) {
          parameters.Param[0]=new EncoderParameter(System.Drawing.Imaging.Encoder.Quality,95L);
          image.Save(stream,codec,parameters);
        }
        return Convert.ToBase64String(stream.ToArray());
      }
    } finally {
      if(bitmap!=IntPtr.Zero) DeleteObject(bitmap);
      if(pidl!=IntPtr.Zero) Marshal.FreeCoTaskMem(pidl);
      if(unknown!=IntPtr.Zero) Marshal.Release(unknown);
      if(factory!=null && Marshal.IsComObject(factory)) Marshal.FinalReleaseComObject(factory);
    }
  }
}
'@
$requests=ConvertFrom-Json $env:NPS_THUMB_REQUESTS
$shell=New-Object -ComObject Shell.Application
$result=New-Object System.Collections.Generic.List[object]
foreach($request in @($requests)) {
  try {
    $folder=$shell.Namespace(17); $item=$null
    foreach($part in @($request.path -split '\\')) {
      $item=@($folder.Items() | Where-Object {$_.Name -eq $part})[0]
      if(!$item){throw "MTP object not found: $part"}
      if($item.IsFolder){$folder=$item.GetFolder}
    }
    $data=[NpsShellThumbnail]::Read($item,[int]$env:NPS_THUMB_SIZE)
    $result.Add([pscustomobject]@{handle=[uint32]$request.handle;data=$data;error=$null})
  } catch {
    $result.Add([pscustomobject]@{handle=[uint32]$request.handle;data='';error=$_.Exception.Message})
  }
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
        if raw.is_empty() {
            return Ok(Vec::new());
        }
        let names: Vec<String> = serde_json::from_str(&raw)
            .or_else(|_| serde_json::from_str::<String>(&raw).map(|v| vec![v]))
            .map_err(|e| UsbError::Connection(format!("cannot parse Windows device list: {e}")))?;
        Ok(names.into_iter().map(|name| (name.clone(), name)).collect())
    }

    pub fn connect(device_name: &str) -> Result<Self, UsbError> {
        // The device node and its top-level storage can be ready while the
        // nested DCIM/100NZ_* folder still returns an empty Items collection.
        // Build a fresh Shell.Application snapshot a few times before accepting
        // zero supported files. A genuinely empty card settles after this
        // bounded retry; a transient WPD tree no longer becomes a sticky empty
        // catalogue in the application session.
        let enumeration_script = [READ_ONLY_ATTRIBUTE_FUNCTION, ENUM_SCRIPT].join("\n");
        let mut raw = String::new();
        for attempt in 0..4 {
            raw = powershell(&enumeration_script, &[("NPS_WPD_DEVICE", device_name)])?;
            if !raw.is_empty() {
                break;
            }
            if attempt < 3 {
                std::thread::sleep(std::time::Duration::from_millis(350));
            }
        }
        let rows: Vec<ShellPhoto> = if raw.is_empty() {
            Vec::new()
        } else {
            serde_json::from_str(&raw)
                .or_else(|_| serde_json::from_str::<ShellPhoto>(&raw).map(|v| vec![v]))
                .map_err(|e| UsbError::Connection(format!("cannot parse MTP photo list: {e}")))?
        };
        let records = rows
            .into_iter()
            .filter(|row| row.device == device_name)
            .enumerate()
            .map(|(index, row)| {
                let ext = row
                    .name
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let format = match ext.as_str() {
                    "jpg" | "jpeg" => PhotoFormat::JPEG,
                    "nef" | "nrw" => PhotoFormat::NEF,
                    "tif" | "tiff" => PhotoFormat::TIFF,
                    _ => PhotoFormat::Unknown(0),
                };
                UsbPhotoRecord {
                    shell_path: row.path.clone(),
                    photo: CameraPhoto {
                        handle: (index + 1) as u32,
                        filename: row.name,
                        size: row.size.unwrap_or(0),
                        format,
                        date: String::new(),
                        width: 0,
                        height: 0,
                        source_path: Some(row.path),
                        protected: row.protected,
                        can_delete: row.can_delete,
                    },
                }
            })
            .collect();
        Ok(Self {
            device_name: device_name.to_string(),
            records,
        })
    }

    pub fn records(&self) -> &[UsbPhotoRecord] {
        &self.records
    }

    pub fn thumbnails(
        records: &[UsbPhotoRecord],
        size: u32,
    ) -> Result<Vec<UsbThumbnail>, UsbError> {
        if records.is_empty() {
            return Ok(Vec::new());
        }
        let requests: Vec<serde_json::Value> = records
            .iter()
            .map(|record| {
                serde_json::json!({
                    "handle": record.photo.handle,
                    "path": record.shell_path,
                })
            })
            .collect();
        let json = serde_json::to_string(&requests)
            .map_err(|error| UsbError::Connection(error.to_string()))?;
        let size_text = size.clamp(96, 1024).to_string();
        let raw = powershell(
            THUMBNAIL_SCRIPT,
            &[
                ("NPS_THUMB_REQUESTS", &json),
                ("NPS_THUMB_SIZE", &size_text),
            ],
        )?;
        if raw.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<ShellThumbnail> = serde_json::from_str(&raw)
            .or_else(|_| serde_json::from_str::<ShellThumbnail>(&raw).map(|value| vec![value]))
            .map_err(|error| {
                UsbError::Connection(format!("cannot parse Windows thumbnails: {error}"))
            })?;
        let mut output = Vec::new();
        let mut failures = Vec::new();
        for row in rows {
            if is_jpeg_thumbnail_payload(&row.data) {
                output.push(UsbThumbnail {
                    handle: row.handle,
                    data: row.data,
                });
            } else {
                failures.push(
                    row.error.unwrap_or_else(|| {
                        "Windows returned no true thumbnail resource".to_string()
                    }),
                );
            }
        }
        if !failures.is_empty() {
            tracing::warn!(
                requested = records.len(),
                returned = output.len(),
                first_error = %failures[0],
                "some USB camera objects do not expose a real thumbnail"
            );
        }
        // Missing thumbnails are not replaced with a generic icon. Returning
        // the successful subset also lets the UI retry transient WPD misses.
        Ok(output)
    }

    pub fn download(record: &UsbPhotoRecord, dest_path: &str) -> Result<(), UsbError> {
        let destination = std::path::Path::new(dest_path);
        if let Ok(metadata) = std::fs::metadata(destination) {
            if metadata.len() == record.photo.size {
                return Ok(());
            }
            return Err(UsbError::Transfer(format!(
                "destination already exists with a different size: {}",
                destination.display()
            )));
        }
        let script = r#"
$ErrorActionPreference='Stop'
$shell=New-Object -ComObject Shell.Application
$parts=$env:NPS_WPD_PATH -split '\\'
$folder=$shell.Namespace(17)
$item=$null
foreach($part in $parts){ $item=@($folder.Items() | Where-Object {$_.Name -eq $part})[0]; if(!$item){throw "MTP object not found: $part"}; if($item.IsFolder){$folder=$item.GetFolder} }
$dest=[IO.Path]::GetDirectoryName($env:NPS_DEST)
$name=[IO.Path]::GetFileName($env:NPS_DEST)
$destFolder=$shell.Namespace($dest); $destFolder.CopyHere($item,20)
$copied=Join-Path $dest $item.Name
for($i=0;$i -lt 6000;$i++){
  if(Test-Path -LiteralPath $copied){
    $length=(Get-Item -LiteralPath $copied).Length
    if(([uint64]$env:NPS_EXPECTED_SIZE -eq 0) -or ($length -eq [uint64]$env:NPS_EXPECTED_SIZE)){break}
  }
  Start-Sleep -Milliseconds 100
}
if(!(Test-Path -LiteralPath $copied)){throw 'MTP copy timed out before a destination file appeared'}
$actual=(Get-Item -LiteralPath $copied).Length
if(([uint64]$env:NPS_EXPECTED_SIZE -ne 0) -and ($actual -ne [uint64]$env:NPS_EXPECTED_SIZE)){throw "MTP copy incomplete: $actual / $env:NPS_EXPECTED_SIZE bytes"}
if($item.Name -ne $name){Move-Item -LiteralPath $copied -Destination $env:NPS_DEST -Force}
"#;
        let expected_size = record.photo.size.to_string();
        powershell(
            script,
            &[
                ("NPS_WPD_PATH", &record.shell_path),
                ("NPS_DEST", dest_path),
                ("NPS_EXPECTED_SIZE", &expected_size),
            ],
        )?;
        let actual = std::fs::metadata(destination)
            .map_err(|error| UsbError::Transfer(error.to_string()))?
            .len();
        if record.photo.size > 0 && actual != record.photo.size {
            return Err(UsbError::Transfer(format!(
                "downloaded byte count mismatch: {actual} / {}",
                record.photo.size
            )));
        }
        Ok(())
    }

    pub fn delete(record: &UsbPhotoRecord) -> Result<(), UsbError> {
        if record.photo.protected || !record.photo.can_delete {
            return Err(UsbError::Transfer(format!(
                "{} is protected or the camera does not allow deletion",
                record.photo.filename
            )));
        }
        let script = r#"
$ErrorActionPreference='Stop'
$shell=New-Object -ComObject Shell.Application
$parts=@($env:NPS_WPD_PATH -split '\\')
$folder=$shell.Namespace(17)
$item=$null
for($index=0;$index -lt $parts.Count;$index++){
  $item=@($folder.Items() | Where-Object {$_.Name -eq $parts[$index]})[0]
  if(!$item){throw "MTP object not found: $($parts[$index])"}
  if($index -lt $parts.Count-1){$folder=$item.GetFolder}
}
$attributes=$item.ExtendedProperty('System.FileAttributes')
if(TestNpsReadOnlyAttribute $attributes){throw 'OBJECT_WRITE_PROTECTED'}
$deleteVerb=$null
foreach($verb in @($item.Verbs())) {
  $verbName=("$($verb.Name)" -replace '&','').Trim()
  if($verbName -match '(?i)^(delete|remove)(\s|$)|删除|刪除'){$deleteVerb=$verb;break}
}
if($null -eq $deleteVerb){throw 'DELETE_VERB_NOT_AVAILABLE'}
$name=$item.Name
$deleteVerb.DoIt()
for($i=0;$i -lt 600;$i++){
  $remaining=@($folder.Items() | Where-Object {$_.Name -eq $name})[0]
  if(!$remaining){exit 0}
  Start-Sleep -Milliseconds 100
}
throw 'DELETE_CONFIRMATION_TIMEOUT'
"#;
        let delete_script = [READ_ONLY_ATTRIBUTE_FUNCTION, script].join("\n");
        match powershell(&delete_script, &[("NPS_WPD_PATH", &record.shell_path)]) {
            Ok(_) => Ok(()),
            Err(UsbError::Connection(message)) if message.contains("OBJECT_WRITE_PROTECTED") => {
                Err(UsbError::Transfer(format!(
                    "{} is protected on the camera; remove its Protect flag before deleting",
                    record.photo.filename
                )))
            }
            Err(UsbError::Connection(message)) if message.contains("DELETE_VERB_NOT_AVAILABLE") => {
                Err(UsbError::Transfer(format!(
                    "Windows reports that {} cannot be deleted from this camera connection",
                    record.photo.filename
                )))
            }
            Err(UsbError::Connection(message))
                if message.contains("DELETE_CONFIRMATION_TIMEOUT") =>
            {
                Err(UsbError::Transfer(format!(
                    "the camera did not confirm deletion of {} within 60 seconds",
                    record.photo.filename
                )))
            }
            Err(UsbError::Connection(message)) => Err(UsbError::Transfer(if message.is_empty() {
                format!("the camera refused to delete {}", record.photo.filename)
            } else {
                message
            })),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nikon_vendor_id_is_stable() {
        assert_eq!(0x04B0u16, 0x04B0);
    }

    #[test]
    fn accepts_only_the_jpeg_payload_emitted_by_the_thumbnail_bridge() {
        let jpeg = format!("/9j/{}", "A".repeat(512));
        assert!(is_jpeg_thumbnail_payload(&jpeg));
        assert!(!is_jpeg_thumbnail_payload(""));
        assert!(!is_jpeg_thumbnail_payload("iVBORw0KGgoAAA"));
        assert!(!is_jpeg_thumbnail_payload("camera-icon"));
    }

    #[cfg(windows)]
    #[test]
    fn thumbnail_bridge_compiles_without_a_camera() {
        powershell(
            THUMBNAIL_SCRIPT,
            &[("NPS_THUMB_REQUESTS", "[]"), ("NPS_THUMB_SIZE", "320")],
        )
        .expect("PowerShell thumbnail bridge should compile");
    }

    #[cfg(windows)]
    #[test]
    fn enumeration_bridge_does_not_hide_a_missing_device_as_an_empty_card() {
        let script = [READ_ONLY_ATTRIBUTE_FUNCTION, ENUM_SCRIPT].join("\n");
        let error = powershell(
            &script,
            &[("NPS_WPD_DEVICE", "__NPS_INTENTIONALLY_MISSING_DEVICE__")],
        )
        .expect_err("an absent device must be reported as an enumeration error");
        match error {
            UsbError::Connection(message) => assert!(
                message.contains("MTP device disappeared before storage enumeration"),
                "PowerShell should preserve the intended WPD diagnostic, got: {message}"
            ),
            other => panic!("unexpected error type: {other}"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn powershell_read_only_helper_accepts_numeric_and_wpd_letter_flags() {
        let values = serde_json::json!([
            0,
            1,
            "0",
            "1",
            "N",
            "RN",
            "R",
            "Normal",
            "Archive",
            "ReadOnly, Archive",
            null
        ]);
        let body = r#"
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new()
$values=ConvertFrom-Json -InputObject $env:NPS_ATTRIBUTE_VALUES
$result=New-Object System.Collections.Generic.List[bool]
foreach($value in $values){$result.Add([bool](TestNpsReadOnlyAttribute $value))}
ConvertTo-Json -InputObject ($result.ToArray()) -Compress
"#;
        let script = [READ_ONLY_ATTRIBUTE_FUNCTION, body].join("\n");
        let raw = powershell(&script, &[("NPS_ATTRIBUTE_VALUES", &values.to_string())])
            .expect("PowerShell file-attribute helper should run");
        let actual: Vec<bool> = serde_json::from_str(&raw)
            .expect("PowerShell file-attribute helper should return JSON booleans");
        assert_eq!(
            actual,
            vec![false, true, false, true, false, true, true, false, false, true, false]
        );
    }

    #[test]
    fn enumerate_connected_nikon_when_available() {
        let devices = UsbCamera::scan_devices().expect("Windows portable-device scan");
        let Some((path, name)) = devices.first() else {
            eprintln!("No Nikon MTP/PTP camera connected; skipping integration assertion");
            return;
        };
        let camera = UsbCamera::connect(path).expect("enumerate connected Nikon storage");
        eprintln!("{}: {} supported photos", name, camera.records().len());
        assert!(
            !camera.records().is_empty(),
            "Windows exposes the Nikon camera but no supported photos were returned"
        );
        assert!(
            camera.records()[0].photo.size > 0,
            "MTP System.Size should expose the real file size"
        );
    }
}
