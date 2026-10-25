//! PTP/IP protocol implementation for Nikon cameras.
//! Ported from NikonPtpIpClient.java (nikon-image-transmission-main).
//!
//! Protocol flow:
//! 1. Init_Command_Request (1) -> Init_Command_Ack (2)
//! 2. Init_Event_Request (3) -> Init_Event_Ack (4) on a second TCP socket
//! 3. Open_Session (0x1002, transaction ID 0)
//! 4. Get_Object_Handles (0x1007)
//! 5. Get_Object_Info (0x1008) / Get_Thumb (0x100A) / Get_Object (0x1009)

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{self, Cursor, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use thiserror::Error;

use super::{CameraPhoto, PhotoFormat};

#[derive(Error, Debug)]
pub enum PtpError {
    #[error("Connection failed: {0}")]
    Connection(String),
    #[error("Protocol error: {0}")]
    Protocol(String),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Session error: {0}")]
    Session(String),
    #[error("Timeout")]
    Timeout,
}

// PTP/IP packet types
const PTPIP_INIT_COMMAND_REQUEST: u32 = 1;
const PTPIP_INIT_COMMAND_ACK: u32 = 2;
const PTPIP_INIT_EVENT_REQUEST: u32 = 3;
const PTPIP_INIT_EVENT_ACK: u32 = 4;
const PTPIP_INIT_FAIL: u32 = 5;
const PTPIP_COMMAND_REQUEST: u32 = 6;
const PTPIP_COMMAND_RESPONSE: u32 = 7;
const PTPIP_START_DATA_PACKET: u32 = 9;
const PTPIP_DATA_PACKET: u32 = 10;
const PTPIP_END_DATA_PACKET: u32 = 12;

// PTP operation codes
const PTP_OPEN_SESSION: u16 = 0x1002;
const PTP_CLOSE_SESSION: u16 = 0x1003;
const PTP_GET_DEVICE_INFO: u16 = 0x1001;
const PTP_GET_STORAGE_IDS: u16 = 0x1004;
const PTP_GET_OBJECT_HANDLES: u16 = 0x1007;
const PTP_GET_OBJECT_INFO: u16 = 0x1008;
const PTP_GET_OBJECT: u16 = 0x1009;
const PTP_GET_THUMB: u16 = 0x100A;
const PTP_DELETE_OBJECT: u16 = 0x100B;

// PTP response codes
const PTP_OK: u16 = 0x2001;
const PTP_SESSION_NOT_OPEN: u16 = 0x2003;
const PTP_OPERATION_NOT_SUPPORTED: u16 = 0x2005;
const PTP_INVALID_OBJECT_HANDLE: u16 = 0x2009;
const PTP_OBJECT_WRITE_PROTECTED: u16 = 0x200D;
const PTP_STORE_READ_ONLY: u16 = 0x200E;
const PTP_ACCESS_DENIED: u16 = 0x200F;
const PTP_PARTIAL_DELETION: u16 = 0x2012;
const PTP_STORE_NOT_AVAILABLE: u16 = 0x2013;
const PTP_DEVICE_BUSY: u16 = 0x2019;
const PTP_SESSION_ALREADY_OPEN: u16 = 0x201E;

// ProtectionStatus values from PTP and the MTP extension. Nikon PTP/IP image
// objects normally use the two standard values, but handling the MTP values
// keeps the mapping safe if a body exposes them over the same transport.
const PTP_PROTECTION_NONE: u16 = 0x0000;
const PTP_PROTECTION_READ_ONLY: u16 = 0x0001;
const MTP_PROTECTION_READ_ONLY: u16 = 0x8001;
const MTP_PROTECTION_READ_ONLY_DATA: u16 = 0x8002;
const MTP_PROTECTION_NON_TRANSFERABLE_DATA: u16 = 0x8003;

/// Default Nikon AP mode address
pub const DEFAULT_AP_HOST: &str = "192.168.1.1";
pub const DEFAULT_PTPIP_PORT: u16 = 15740;

pub struct PtpIpClient {
    cmd_stream: Arc<TcpStream>,
    // PTP/IP requires the event socket to remain alive for the lifetime of the
    // command session even when the browser does not actively consume events.
    event_stream: Option<Arc<TcpStream>>,
    session_id: u32,
    transaction_id: u32,
    guid: [u8; 16],
    cancelled: Arc<AtomicBool>,
}

/// A cancellation-only view of a PTP/IP connection.
///
/// `TcpStream::try_clone` refers to the same underlying Windows socket. Calling
/// `shutdown` on these clones therefore wakes a command that is blocked in
/// `read_exact`, without first acquiring the mutex that protects the serial
/// PTP transaction stream.
pub struct PtpIpAbortHandle {
    command_stream: Arc<TcpStream>,
    event_stream: Option<Arc<TcpStream>>,
    cancelled: Arc<AtomicBool>,
}

impl PtpIpAbortHandle {
    pub fn abort(self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.command_stream.shutdown(Shutdown::Both);
        if let Some(event_stream) = self.event_stream {
            let _ = event_stream.shutdown(Shutdown::Both);
        }
    }
}

impl PtpIpClient {
    /// Connect to camera via PTP/IP
    pub fn connect(host: &str, port: u16) -> Result<Self, PtpError> {
        let addr: SocketAddr = format!("{}:{}", host, port)
            .parse()
            .map_err(|e| PtpError::Connection(format!("Invalid address: {}", e)))?;

        let cmd_stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
            .map_err(|e| PtpError::Connection(format!("Failed to connect: {}", e)))?;
        cmd_stream.set_nodelay(true)?;
        // Windows does not reliably wake a synchronous WSARecv when another
        // thread calls shutdown on the same socket. A short socket timeout plus
        // an atomic cancellation check makes disconnect deterministic while
        // preserving the original 30-second idle timeout at the protocol layer.
        cmd_stream.set_read_timeout(Some(Duration::from_millis(250)))?;
        cmd_stream.set_write_timeout(Some(Duration::from_secs(10)))?;

        let mut client = Self {
            cmd_stream: Arc::new(cmd_stream),
            event_stream: None,
            session_id: 0,
            transaction_id: 0,
            guid: [0u8; 16],
            cancelled: Arc::new(AtomicBool::new(false)),
        };

        client.init_handshake()?;
        Ok(client)
    }

    /// Create a handle which can cancel blocking I/O without borrowing this
    /// client. It is created only after both PTP/IP sockets are established.
    pub fn abort_handle(&self) -> Result<PtpIpAbortHandle, PtpError> {
        Ok(PtpIpAbortHandle {
            command_stream: Arc::clone(&self.cmd_stream),
            event_stream: self.event_stream.as_ref().map(Arc::clone),
            cancelled: Arc::clone(&self.cancelled),
        })
    }

    /// Perform PTP/IP init handshake
    fn init_handshake(&mut self) -> Result<(), PtpError> {
        // Keep the same stable initiator identity as the proven Android
        // reference. A new synthetic GUID on every implementation can make a
        // paired Nikon body treat each connection as an unknown computer.
        self.guid.copy_from_slice(b"NIKONTRANSFER01\0");

        // Build Init_Command_Request packet
        let mut payload = Vec::new();
        payload.extend_from_slice(&self.guid);
        // Initiator name is part of Nikon's pairing identity.
        let name = "Nikon Transfer Android";
        for c in name.encode_utf16() {
            payload.write_u16::<LittleEndian>(c)?;
        }
        payload.write_u16::<LittleEndian>(0)?; // null terminator
        payload.write_u32::<LittleEndian>(0x0001_0000)?; // PTP/IP 1.0

        self.write_packet(PTPIP_INIT_COMMAND_REQUEST, &payload)?;

        // Read Init_Command_Ack
        let (ptype, data) = self.read_packet()?;
        if ptype != PTPIP_INIT_COMMAND_ACK {
            return Err(PtpError::Protocol(format!(
                "Expected Init_Command_Ack (2), got packet type {}",
                ptype
            )));
        }

        // Parse ack: connection_id (4 bytes) + guid (16 bytes) + name + version.
        // The connection ID associates the required second (event) socket
        // with this command socket.
        if data.len() < 4 {
            return Err(PtpError::Protocol("Init ack too short".into()));
        }

        let connection_id = Cursor::new(&data[..4]).read_u32::<LittleEndian>()?;
        self.init_event_channel(connection_id)?;
        Ok(())
    }

    fn init_event_channel(&mut self, connection_id: u32) -> Result<(), PtpError> {
        let peer = self.cmd_stream.peer_addr()?;
        let mut last_error = None;
        let mut event_stream = None;

        // A Nikon body can publish the command listener a fraction earlier
        // than it is ready to accept the associated event socket.
        for attempt in 0..3 {
            match TcpStream::connect_timeout(&peer, Duration::from_secs(5)) {
                Ok(stream) => {
                    event_stream = Some(stream);
                    break;
                }
                Err(error) => {
                    last_error = Some(error);
                    if attempt < 2 {
                        std::thread::sleep(Duration::from_millis(150));
                    }
                }
            }
        }

        let mut stream = event_stream.ok_or_else(|| {
            let detail = last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "unknown error".to_string());
            PtpError::Connection(format!("Failed to open PTP/IP event channel: {detail}"))
        })?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;

        let mut payload = Vec::with_capacity(4);
        payload.write_u32::<LittleEndian>(connection_id)?;
        let packet = Self::build_packet(PTPIP_INIT_EVENT_REQUEST, &payload);
        stream.write_all(&packet)?;
        stream.flush()?;

        let (packet_type, data) = Self::read_packet_from(&mut stream, "event handshake")
            .map_err(Self::map_event_handshake_error)?;
        if packet_type == PTPIP_INIT_FAIL {
            let reason = data
                .get(..4)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u32::from_le_bytes)
                .unwrap_or_default();
            return Err(PtpError::Protocol(format!(
                "Camera rejected the PTP/IP event channel (reason 0x{reason:08X})"
            )));
        }
        if packet_type != PTPIP_INIT_EVENT_ACK {
            return Err(PtpError::Protocol(format!(
                "Expected Init_Event_Ack (4), got packet type {packet_type}"
            )));
        }

        self.event_stream = Some(Arc::new(stream));
        Ok(())
    }

    fn map_event_handshake_error(error: PtpError) -> PtpError {
        match error {
            PtpError::Io(io_error) if io_error.kind() == io::ErrorKind::UnexpectedEof => {
                PtpError::Protocol("Camera closed the event channel before Init_Event_Ack".into())
            }
            other => other,
        }
    }

    /// Open a PTP session
    pub fn open_session(&mut self) -> Result<u32, PtpError> {
        // PTP requires OpenSession to use transaction ID 0. The first command
        // after a successful OpenSession increments this field to 1.
        let tid = 0;

        let mut params = Vec::new();
        params.write_u32::<LittleEndian>(1)?; // session ID = 1

        let resp =
            self.send_command(PTP_OPEN_SESSION, tid, &params)
                .map_err(|error| match error {
                    PtpError::Io(io_error) if io_error.kind() == io::ErrorKind::UnexpectedEof => {
                        PtpError::Session(
                            "Camera closed the PTP/IP command channel while opening the session"
                                .into(),
                        )
                    }
                    other => other,
                })?;
        match resp.code {
            PTP_OK | PTP_SESSION_ALREADY_OPEN => {
                self.session_id = 1;
                Ok(1)
            }
            code => Err(PtpError::Session(format!(
                "OpenSession failed: 0x{:04X}",
                code
            ))),
        }
    }

    pub fn get_device_model(&mut self) -> Result<String, PtpError> {
        self.transaction_id += 1;
        let tid = self.transaction_id;
        let resp = self.send_command(PTP_GET_DEVICE_INFO, tid, &[])?;
        if resp.code != PTP_OK {
            return Err(PtpError::Protocol(format!(
                "GetDeviceInfo failed: 0x{:04X}",
                resp.code
            )));
        }
        let data = resp
            .data
            .ok_or_else(|| PtpError::Protocol("GetDeviceInfo returned no data".into()))?;
        let mut cursor = Cursor::new(data.as_slice());
        let _standard = cursor.read_u16::<LittleEndian>()?;
        let _vendor = cursor.read_u32::<LittleEndian>()?;
        let _vendor_version = cursor.read_u16::<LittleEndian>()?;
        let _description = Self::read_ptp_string(&mut cursor)?;
        let _mode = cursor.read_u16::<LittleEndian>()?;
        for _ in 0..5 {
            let count = cursor.read_u32::<LittleEndian>()? as u64;
            cursor.set_position(cursor.position().saturating_add(count * 2));
        }
        let manufacturer = Self::read_ptp_string(&mut cursor)?.unwrap_or_else(|| "Nikon".into());
        let model = Self::read_ptp_string(&mut cursor)?.unwrap_or_else(|| "Camera".into());
        Ok(if model.to_ascii_lowercase().contains("nikon") {
            model
        } else {
            format!("{manufacturer} {model}")
        })
    }

    fn read_ptp_string(cursor: &mut Cursor<&[u8]>) -> Result<Option<String>, PtpError> {
        let len = cursor.read_u8()? as usize;
        if len == 0 {
            return Ok(None);
        }
        let mut chars = Vec::with_capacity(len);
        for _ in 0..len {
            chars.push(cursor.read_u16::<LittleEndian>()?);
        }
        while chars.last() == Some(&0) {
            chars.pop();
        }
        Ok(String::from_utf16(&chars).ok())
    }

    /// Get all object handles from the camera
    pub fn get_object_handles(&mut self) -> Result<Vec<u32>, PtpError> {
        let storage_ids = match self.get_storage_ids() {
            Ok(ids) if !ids.is_empty() => ids,
            _ => vec![0xFFFF_FFFF],
        };
        let mut all = Vec::new();
        for storage_id in storage_ids {
            if let Ok(handles) = self.get_object_handles_for_storage(storage_id) {
                all.extend(handles);
            }
        }
        if all.is_empty() {
            all = self.get_object_handles_for_storage(0xFFFF_FFFF)?;
        }
        all.sort_unstable();
        all.dedup();
        Ok(all)
    }

    fn get_storage_ids(&mut self) -> Result<Vec<u32>, PtpError> {
        self.transaction_id += 1;
        let tid = self.transaction_id;
        let resp = self.send_command(PTP_GET_STORAGE_IDS, tid, &[])?;
        if resp.code != PTP_OK {
            return Err(PtpError::Protocol(format!(
                "GetStorageIDs failed: 0x{:04X}",
                resp.code
            )));
        }
        let data = resp
            .data
            .ok_or_else(|| PtpError::Protocol("GetStorageIDs returned no data".into()))?;
        let mut cursor = Cursor::new(&data);
        let count = cursor.read_u32::<LittleEndian>()?;
        let mut ids = Vec::with_capacity(count as usize);
        for _ in 0..count {
            ids.push(cursor.read_u32::<LittleEndian>()?);
        }
        Ok(ids)
    }

    fn get_object_handles_for_storage(&mut self, storage_id: u32) -> Result<Vec<u32>, PtpError> {
        self.transaction_id += 1;
        let tid = self.transaction_id;

        let mut params = Vec::new();
        params.write_u32::<LittleEndian>(storage_id)?;
        params.write_u32::<LittleEndian>(0)?; // format: all
        params.write_u32::<LittleEndian>(0)?; // association: none

        let resp = self.send_command(PTP_GET_OBJECT_HANDLES, tid, &params)?;
        if resp.code != PTP_OK {
            return Err(PtpError::Protocol(format!(
                "GetObjectHandles failed: 0x{:04X}",
                resp.code
            )));
        }

        // Parse handle array from data phase
        let data = resp
            .data
            .ok_or_else(|| PtpError::Protocol("No data phase".into()))?;
        let mut cursor = Cursor::new(&data);
        let count = cursor.read_u32::<LittleEndian>()?;
        let mut handles = Vec::with_capacity(count as usize);
        for _ in 0..count {
            handles.push(cursor.read_u32::<LittleEndian>()?);
        }
        Ok(handles)
    }

    /// Get object info for a specific handle
    pub fn get_object_info(&mut self, handle: u32) -> Result<CameraPhoto, PtpError> {
        self.transaction_id += 1;
        let tid = self.transaction_id;

        let mut params = Vec::new();
        params.write_u32::<LittleEndian>(handle)?;

        let resp = self.send_command(PTP_GET_OBJECT_INFO, tid, &params)?;
        if resp.code != PTP_OK {
            return Err(PtpError::Protocol(format!(
                "GetObjectInfo failed: 0x{:04X}",
                resp.code
            )));
        }

        let data = resp
            .data
            .ok_or_else(|| PtpError::Protocol("No data".into()))?;
        Self::parse_object_info(&data, handle)
    }

    /// Download full object data
    pub fn get_object(&mut self, handle: u32) -> Result<Vec<u8>, PtpError> {
        self.transaction_id += 1;
        let tid = self.transaction_id;

        let mut params = Vec::new();
        params.write_u32::<LittleEndian>(handle)?;

        let resp = self.send_command(PTP_GET_OBJECT, tid, &params)?;
        if resp.code != PTP_OK {
            return Err(PtpError::Protocol(format!(
                "GetObject failed: 0x{:04X}",
                resp.code
            )));
        }

        resp.data
            .ok_or_else(|| PtpError::Protocol("No object data".into()))
    }

    /// Stream an original object directly to a writer.
    ///
    /// PTP/IP is a strictly ordered session, so this still uses exactly one
    /// transaction on the command socket. Unlike `get_object`, it never keeps
    /// a second 20-30 MB RAW-sized buffer in memory. The callback is invoked as
    /// chunks arrive and receives `(received, camera_declared_total)`.
    pub fn get_object_to_writer<W, F>(
        &mut self,
        handle: u32,
        writer: &mut W,
        progress: F,
    ) -> Result<u64, PtpError>
    where
        W: Write,
        F: FnMut(u64, u64),
    {
        self.transaction_id += 1;
        let tid = self.transaction_id;
        let mut params = Vec::with_capacity(4);
        params.write_u32::<LittleEndian>(handle)?;
        self.write_command_request(PTP_GET_OBJECT, tid, &params)?;
        let response = self.read_response_to_writer(tid, writer, progress)?;
        if response.code != PTP_OK {
            return Err(PtpError::Protocol(format!(
                "GetObject failed: 0x{:04X}",
                response.code
            )));
        }
        if !response.saw_data {
            return Err(PtpError::Protocol(
                "GetObject returned no data phase".into(),
            ));
        }
        Ok(response.bytes_received)
    }

    /// Get thumbnail for a handle
    pub fn get_thumbnail(&mut self, handle: u32) -> Result<Vec<u8>, PtpError> {
        self.transaction_id += 1;
        let tid = self.transaction_id;

        let mut params = Vec::new();
        params.write_u32::<LittleEndian>(handle)?;

        let resp = self.send_command(PTP_GET_THUMB, tid, &params)?;
        if resp.code != PTP_OK {
            return Err(PtpError::Protocol(format!(
                "GetThumb failed: 0x{:04X}",
                resp.code
            )));
        }

        resp.data
            .ok_or_else(|| PtpError::Protocol("No thumb data".into()))
    }

    /// Permanently delete one object from the camera storage.
    ///
    /// This deliberately accepts exactly one concrete object handle. PTP
    /// reserves `0xFFFF_FFFF` for "all objects", so rejecting reserved handles
    /// here prevents a UI or serialization bug from erasing the entire card.
    pub fn delete_object(&mut self, handle: u32) -> Result<(), PtpError> {
        if handle == 0 || handle == u32::MAX {
            return Err(PtpError::Protocol(format!(
                "拒绝危险的删除句柄 0x{handle:08X}；只能逐张删除相机照片"
            )));
        }

        // Re-read ObjectInfo immediately before deletion instead of trusting a
        // potentially stale grid item. The responder remains the final source
        // of truth and can still return ObjectWriteProtected below if its state
        // changes between these two transactions.
        let photo = self.get_object_info(handle)?;
        if !photo.can_delete {
            return Err(PtpError::Protocol(format!(
                "照片 {} 已在相机中受保护，无法删除",
                photo.filename
            )));
        }

        self.transaction_id += 1;
        let tid = self.transaction_id;
        let mut params = Vec::new();
        params.write_u32::<LittleEndian>(handle)?;
        params.write_u32::<LittleEndian>(0)?; // no ObjectFormatCode filter

        let response = self.send_command(PTP_DELETE_OBJECT, tid, &params)?;
        match response.code {
            PTP_OK => Ok(()),
            PTP_OBJECT_WRITE_PROTECTED => Err(PtpError::Protocol(
                "照片已在相机中受保护，无法删除 (PTP 0x200D)".into(),
            )),
            PTP_STORE_READ_ONLY => Err(PtpError::Protocol(
                "相机存储卡为只读状态，无法删除 (PTP 0x200E)".into(),
            )),
            PTP_ACCESS_DENIED => Err(PtpError::Protocol(
                "相机或系统拒绝删除权限 (PTP 0x200F)".into(),
            )),
            PTP_OPERATION_NOT_SUPPORTED => Err(PtpError::Protocol(
                "当前相机模式不支持远程删除 (PTP 0x2005)".into(),
            )),
            PTP_INVALID_OBJECT_HANDLE => Err(PtpError::Protocol(
                "照片句柄已失效，请重新扫描相机 (PTP 0x2009)".into(),
            )),
            PTP_PARTIAL_DELETION => Err(PtpError::Protocol(
                "相机仅完成了部分删除，请重新扫描确认 (PTP 0x2012)".into(),
            )),
            PTP_STORE_NOT_AVAILABLE => Err(PtpError::Protocol(
                "相机存储卡当前不可用 (PTP 0x2013)".into(),
            )),
            PTP_DEVICE_BUSY => Err(PtpError::Protocol(
                "相机正忙，暂时无法删除，请稍后重试 (PTP 0x2019)".into(),
            )),
            PTP_SESSION_NOT_OPEN => Err(PtpError::Session(
                "相机会话已关闭，请重新连接后再删除 (PTP 0x2003)".into(),
            )),
            code => Err(PtpError::Protocol(format!(
                "DeleteObject failed: PTP 0x{code:04X}"
            ))),
        }
    }

    /// Close the session
    pub fn close_session(&mut self) -> Result<(), PtpError> {
        self.transaction_id += 1;
        let tid = self.transaction_id;
        let _ = self.send_command(PTP_CLOSE_SESSION, tid, &[]);
        self.session_id = 0;
        Ok(())
    }

    // --- Internal helpers ---

    fn send_command(
        &mut self,
        opcode: u16,
        tid: u32,
        params: &[u8],
    ) -> Result<PtpResponse, PtpError> {
        self.write_command_request(opcode, tid, params)?;

        // Read response (may include data packets)
        self.read_response(tid)
    }

    fn write_command_request(
        &mut self,
        opcode: u16,
        tid: u32,
        params: &[u8],
    ) -> Result<(), PtpError> {
        // Build Command_Request packet
        let mut payload = Vec::with_capacity(10 + params.len());
        // Nikon's PC-transfer implementation uses value 1 for these commands
        // and still returns Start/Data/End packets for read operations. Match
        // the working Android reference byte-for-byte for body compatibility.
        payload.write_u32::<LittleEndian>(1)?;
        payload.write_u16::<LittleEndian>(opcode)?;
        payload.write_u32::<LittleEndian>(tid)?;
        payload.extend_from_slice(params);

        self.write_packet(PTPIP_COMMAND_REQUEST, &payload)
    }

    fn read_response(&mut self, expected_tid: u32) -> Result<PtpResponse, PtpError> {
        let mut data_buf: Option<Vec<u8>> = None;
        let mut expected_data_length = None;

        let response_code = loop {
            let (ptype, data) = self.read_packet()?;
            match ptype {
                PTPIP_START_DATA_PACKET => {
                    // transaction id (4) + total length (8) + optional data
                    if data.len() < 12 {
                        return Err(PtpError::Protocol("StartData packet is too short".into()));
                    }
                    let tid = u32::from_le_bytes(data[0..4].try_into().unwrap());
                    if tid != expected_tid {
                        return Err(PtpError::Protocol(format!(
                            "StartData transaction mismatch: expected {expected_tid}, got {tid}"
                        )));
                    }
                    let total = u64::from_le_bytes(data[4..12].try_into().unwrap());
                    const MAX_IN_MEMORY_DATA: u64 = 512 * 1024 * 1024;
                    if total > MAX_IN_MEMORY_DATA {
                        return Err(PtpError::Protocol(format!(
                            "Data phase is too large for an in-memory operation: {total} bytes"
                        )));
                    }
                    expected_data_length = Some(total);
                    let mut buf = Vec::with_capacity(total as usize);
                    if data.len() > 12 {
                        buf.extend_from_slice(&data[12..]);
                    }
                    data_buf = Some(buf);
                }
                PTPIP_DATA_PACKET => {
                    Self::validate_data_transaction(&data, expected_tid, "Data")?;
                    if let Some(ref mut buf) = data_buf {
                        if data.len() > 4 {
                            buf.extend_from_slice(&data[4..]);
                        }
                    }
                }
                PTPIP_END_DATA_PACKET => {
                    Self::validate_data_transaction(&data, expected_tid, "EndData")?;
                    if let Some(ref mut buf) = data_buf {
                        if data.len() > 4 {
                            buf.extend_from_slice(&data[4..]);
                        }
                    }
                }
                PTPIP_COMMAND_RESPONSE => {
                    // Parse: response_code(2) + transaction_id(4) + params
                    if data.len() < 6 {
                        return Err(PtpError::Protocol(
                            "CommandResponse packet is too short".into(),
                        ));
                    }
                    let mut cursor = Cursor::new(&data);
                    let code = cursor.read_u16::<LittleEndian>()?;
                    let tid = cursor.read_u32::<LittleEndian>()?;
                    if tid != expected_tid {
                        return Err(PtpError::Protocol(format!(
                            "CommandResponse transaction mismatch: expected {expected_tid}, got {tid}"
                        )));
                    }
                    break code;
                }
                other => {
                    return Err(PtpError::Protocol(format!(
                        "Unexpected packet type: {}",
                        other
                    )));
                }
            }
        };

        if response_code == PTP_OK {
            if let (Some(expected), Some(data)) = (expected_data_length, data_buf.as_ref()) {
                if data.len() as u64 != expected {
                    return Err(PtpError::Protocol(format!(
                        "Data transfer was incomplete: received {} of {expected} bytes",
                        data.len()
                    )));
                }
            }
        }

        Ok(PtpResponse {
            code: response_code,
            data: data_buf,
        })
    }

    fn validate_data_transaction(
        payload: &[u8],
        expected_tid: u32,
        packet_name: &str,
    ) -> Result<(), PtpError> {
        if payload.len() < 4 {
            return Err(PtpError::Protocol(format!(
                "{packet_name} packet is too short"
            )));
        }
        let tid = u32::from_le_bytes(payload[0..4].try_into().unwrap());
        if tid != expected_tid {
            return Err(PtpError::Protocol(format!(
                "{packet_name} transaction mismatch: expected {expected_tid}, got {tid}"
            )));
        }
        Ok(())
    }

    fn read_response_to_writer<W, F>(
        &mut self,
        expected_tid: u32,
        writer: &mut W,
        mut progress: F,
    ) -> Result<StreamResponse, PtpError>
    where
        W: Write,
        F: FnMut(u64, u64),
    {
        let mut saw_data = false;
        let mut expected_total = None;
        let mut bytes_received = 0u64;

        let response_code = loop {
            let (packet_type, payload_len) = Self::read_packet_header_from_controlled(
                &self.cmd_stream,
                "command",
                Some(&self.cancelled),
            )?;
            match packet_type {
                PTPIP_START_DATA_PACKET => {
                    if payload_len < 12 {
                        return Err(PtpError::Protocol("StartData packet is too short".into()));
                    }
                    let mut prefix = [0u8; 12];
                    Self::read_exact_controlled(
                        &self.cmd_stream,
                        &mut prefix,
                        Some(&self.cancelled),
                    )?;
                    let tid = u32::from_le_bytes(prefix[0..4].try_into().unwrap());
                    if tid != expected_tid {
                        return Err(PtpError::Protocol(format!(
                            "StartData transaction mismatch: expected {expected_tid}, got {tid}"
                        )));
                    }
                    let total = u64::from_le_bytes(prefix[4..12].try_into().unwrap());
                    // PTP/IP reports the full object size as u64. Bound it to
                    // a practical maximum to reject a corrupt packet before a
                    // multi-gigabyte sparse/partial file is produced.
                    const MAX_STREAMED_OBJECT: u64 = 16 * 1024 * 1024 * 1024;
                    if total > MAX_STREAMED_OBJECT {
                        return Err(PtpError::Protocol(format!(
                            "Camera declared an implausibly large object: {total} bytes"
                        )));
                    }
                    saw_data = true;
                    expected_total = Some(total);
                    progress(0, total);
                    Self::copy_packet_payload(
                        &self.cmd_stream,
                        writer,
                        payload_len - 12,
                        &mut bytes_received,
                        total,
                        &mut progress,
                        Some(&self.cancelled),
                    )?;
                }
                PTPIP_DATA_PACKET | PTPIP_END_DATA_PACKET => {
                    if payload_len < 4 {
                        return Err(PtpError::Protocol(format!(
                            "Data packet is too short: {payload_len} bytes"
                        )));
                    }
                    let mut tid_bytes = [0u8; 4];
                    Self::read_exact_controlled(
                        &self.cmd_stream,
                        &mut tid_bytes,
                        Some(&self.cancelled),
                    )?;
                    let tid = u32::from_le_bytes(tid_bytes);
                    if tid != expected_tid {
                        return Err(PtpError::Protocol(format!(
                            "Data transaction mismatch: expected {expected_tid}, got {tid}"
                        )));
                    }
                    let total = expected_total.ok_or_else(|| {
                        PtpError::Protocol("Data packet arrived before StartData".into())
                    })?;
                    Self::copy_packet_payload(
                        &self.cmd_stream,
                        writer,
                        payload_len - 4,
                        &mut bytes_received,
                        total,
                        &mut progress,
                        Some(&self.cancelled),
                    )?;
                }
                PTPIP_COMMAND_RESPONSE => {
                    let data = Self::read_payload_from_controlled(
                        &self.cmd_stream,
                        payload_len,
                        "command response",
                        Some(&self.cancelled),
                    )?;
                    if data.len() < 6 {
                        return Err(PtpError::Protocol(
                            "CommandResponse packet is too short".into(),
                        ));
                    }
                    let code = u16::from_le_bytes(data[0..2].try_into().unwrap());
                    let tid = u32::from_le_bytes(data[2..6].try_into().unwrap());
                    if tid != expected_tid {
                        return Err(PtpError::Protocol(format!(
                            "CommandResponse transaction mismatch: expected {expected_tid}, got {tid}"
                        )));
                    }
                    break code;
                }
                other => {
                    // Consume the payload before reporting the protocol error
                    // so the diagnostic does not leave a half packet in the
                    // socket if the caller elects to close the session.
                    Self::discard_payload(&self.cmd_stream, payload_len, Some(&self.cancelled))?;
                    return Err(PtpError::Protocol(format!(
                        "Unexpected packet type while streaming object: {other}"
                    )));
                }
            }
        };

        if response_code == PTP_OK {
            if let Some(expected) = expected_total {
                if bytes_received != expected {
                    return Err(PtpError::Protocol(format!(
                        "Object transfer was incomplete: received {bytes_received} of {expected} bytes"
                    )));
                }
                progress(bytes_received, expected);
            }
        }

        Ok(StreamResponse {
            code: response_code,
            saw_data,
            bytes_received,
        })
    }

    fn copy_packet_payload<W, F>(
        stream: &TcpStream,
        writer: &mut W,
        payload_len: usize,
        bytes_received: &mut u64,
        total: u64,
        progress: &mut F,
        cancelled: Option<&AtomicBool>,
    ) -> Result<(), PtpError>
    where
        W: Write,
        F: FnMut(u64, u64),
    {
        let mut remaining = payload_len;
        let mut buffer = [0u8; 64 * 1024];
        while remaining > 0 {
            let chunk_len = remaining.min(buffer.len());
            Self::read_exact_controlled(stream, &mut buffer[..chunk_len], cancelled)?;
            writer.write_all(&buffer[..chunk_len])?;
            *bytes_received = (*bytes_received).saturating_add(chunk_len as u64);
            if *bytes_received > total {
                return Err(PtpError::Protocol(format!(
                    "Camera sent more object data than declared: {} > {total}",
                    *bytes_received
                )));
            }
            progress(*bytes_received, total);
            remaining -= chunk_len;
        }
        Ok(())
    }

    fn discard_payload(
        stream: &TcpStream,
        payload_len: usize,
        cancelled: Option<&AtomicBool>,
    ) -> Result<(), PtpError> {
        let mut remaining = payload_len;
        let mut buffer = [0u8; 16 * 1024];
        while remaining > 0 {
            let chunk = remaining.min(buffer.len());
            Self::read_exact_controlled(stream, &mut buffer[..chunk], cancelled)?;
            remaining -= chunk;
        }
        Ok(())
    }

    fn write_packet(&mut self, ptype: u32, payload: &[u8]) -> Result<(), PtpError> {
        let pkt = Self::build_packet(ptype, payload);
        (&*self.cmd_stream).write_all(&pkt)?;
        (&*self.cmd_stream).flush()?;
        Ok(())
    }

    fn read_packet(&mut self) -> Result<(u32, Vec<u8>), PtpError> {
        Self::read_packet_from_controlled(&self.cmd_stream, "command", Some(&self.cancelled))
    }

    fn read_packet_from(
        stream: &TcpStream,
        channel_name: &str,
    ) -> Result<(u32, Vec<u8>), PtpError> {
        Self::read_packet_from_controlled(stream, channel_name, None)
    }

    fn read_packet_from_controlled(
        stream: &TcpStream,
        channel_name: &str,
        cancelled: Option<&AtomicBool>,
    ) -> Result<(u32, Vec<u8>), PtpError> {
        let (ptype, payload_len) =
            Self::read_packet_header_from_controlled(stream, channel_name, cancelled)?;
        let payload =
            Self::read_payload_from_controlled(stream, payload_len, channel_name, cancelled)?;
        Ok((ptype, payload))
    }

    fn read_packet_header_from(
        stream: &TcpStream,
        channel_name: &str,
    ) -> Result<(u32, usize), PtpError> {
        Self::read_packet_header_from_controlled(stream, channel_name, None)
    }

    fn read_packet_header_from_controlled(
        stream: &TcpStream,
        channel_name: &str,
        cancelled: Option<&AtomicBool>,
    ) -> Result<(u32, usize), PtpError> {
        let mut header = [0u8; 8];
        Self::read_exact_controlled(stream, &mut header, cancelled)?;
        let mut cursor = Cursor::new(&header);
        let length = cursor.read_u32::<LittleEndian>()?;
        let ptype = cursor.read_u32::<LittleEndian>()?;

        if length < 8 {
            return Err(PtpError::Protocol(format!(
                "Invalid {channel_name} packet length: {length}"
            )));
        }

        let payload_len = (length - 8) as usize;
        const MAX_PACKET_SIZE: usize = 128 * 1024 * 1024;
        if payload_len > MAX_PACKET_SIZE {
            return Err(PtpError::Protocol(format!(
                "{channel_name} packet is too large: {payload_len} bytes"
            )));
        }
        Ok((ptype, payload_len))
    }

    fn read_payload_from(
        stream: &TcpStream,
        payload_len: usize,
        channel_name: &str,
    ) -> Result<Vec<u8>, PtpError> {
        Self::read_payload_from_controlled(stream, payload_len, channel_name, None)
    }

    fn read_payload_from_controlled(
        stream: &TcpStream,
        payload_len: usize,
        _channel_name: &str,
        cancelled: Option<&AtomicBool>,
    ) -> Result<Vec<u8>, PtpError> {
        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 {
            Self::read_exact_controlled(stream, &mut payload, cancelled)?;
        }
        Ok(payload)
    }

    fn read_exact_controlled(
        stream: &TcpStream,
        mut buffer: &mut [u8],
        cancelled: Option<&AtomicBool>,
    ) -> Result<(), PtpError> {
        if cancelled.is_none() {
            (&*stream).read_exact(buffer)?;
            return Ok(());
        }

        let cancelled = cancelled.expect("checked above");
        let mut last_progress = Instant::now();
        while !buffer.is_empty() {
            if cancelled.load(Ordering::Acquire) {
                return Err(PtpError::Io(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "PTP/IP operation cancelled",
                )));
            }
            match (&*stream).read(buffer) {
                Ok(0) => {
                    return Err(PtpError::Io(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "PTP/IP socket closed",
                    )))
                }
                Ok(read) => {
                    let (_, rest) = std::mem::take(&mut buffer).split_at_mut(read);
                    buffer = rest;
                    last_progress = Instant::now();
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    if last_progress.elapsed() >= Duration::from_secs(30) {
                        return Err(PtpError::Io(error));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(PtpError::Io(error)),
            }
        }
        Ok(())
    }

    fn build_packet(ptype: u32, payload: &[u8]) -> Vec<u8> {
        let length = 8 + payload.len();
        let mut pkt = Vec::with_capacity(length);
        pkt.write_u32::<LittleEndian>(length as u32).unwrap();
        pkt.write_u32::<LittleEndian>(ptype).unwrap();
        pkt.extend_from_slice(payload);
        pkt
    }

    fn parse_object_info(data: &[u8], handle: u32) -> Result<CameraPhoto, PtpError> {
        // ObjectInfo dataset:
        // storage_id(4) + format(2) + protection(2) + compressed_size(4) +
        // thumb_format(2) + thumb_compressed_size(4) + thumb_pix_width(4) + thumb_pix_height(4) +
        // image_pix_width(4) + image_pix_height(4) + image_bit_depth(4) + parent(4) +
        // association_type(2) + association_desc(4) + sequence_number(4) +
        // filename(variable UTF-16) + capture_date + modification_date + keywords
        let mut cursor = Cursor::new(data);

        let _storage_id = cursor.read_u32::<LittleEndian>()?;
        let format_code = cursor.read_u16::<LittleEndian>()?;
        let protection_status = cursor.read_u16::<LittleEndian>()?;
        let compressed_size = cursor.read_u32::<LittleEndian>()?;
        let _thumb_format = cursor.read_u16::<LittleEndian>()?;
        let _thumb_size = cursor.read_u32::<LittleEndian>()?;
        let _thumb_w = cursor.read_u32::<LittleEndian>()?;
        let _thumb_h = cursor.read_u32::<LittleEndian>()?;
        let width = cursor.read_u32::<LittleEndian>()?;
        let height = cursor.read_u32::<LittleEndian>()?;

        // Consume the remaining fixed fields before the first PTP string.
        let _bit_depth = cursor.read_u32::<LittleEndian>()?;
        let _parent = cursor.read_u32::<LittleEndian>()?;
        let _association_type = cursor.read_u16::<LittleEndian>()?;
        let _association_desc = cursor.read_u32::<LittleEndian>()?;
        let _sequence = cursor.read_u32::<LittleEndian>()?;

        // Skip to filename (position after fixed fields)
        let pos = cursor.position() as usize;
        let remaining = &data[pos..];

        // Parse filename (length-prefixed UTF-16LE string)
        let filename =
            Self::read_utf16_string(remaining).unwrap_or_else(|| format!("OBJ_{:08X}", handle));

        let (protected, can_delete) = Self::map_protection_status(protection_status);

        Ok(CameraPhoto {
            handle,
            filename,
            size: compressed_size as u64,
            format: PhotoFormat::from(format_code),
            date: String::new(),
            width,
            height,
            source_path: None,
            protected,
            can_delete,
        })
    }

    fn map_protection_status(status: u16) -> (bool, bool) {
        match status {
            PTP_PROTECTION_NONE | MTP_PROTECTION_NON_TRANSFERABLE_DATA => (false, true),
            PTP_PROTECTION_READ_ONLY | MTP_PROTECTION_READ_ONLY | MTP_PROTECTION_READ_ONLY_DATA => {
                (true, false)
            }
            // ProtectionStatus is extensible. Unknown non-zero values are
            // conservatively treated as protected so the application never
            // probes support by issuing a destructive request.
            _ => (true, false),
        }
    }

    fn read_utf16_string(data: &[u8]) -> Option<String> {
        if data.is_empty() {
            return None;
        }
        let char_count = data[0] as usize;
        if char_count == 0 || data.len() < 1 + char_count * 2 {
            return None;
        }
        let utf16_data = &data[1..1 + char_count * 2];
        let chars: Vec<u16> = utf16_data
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        // Remove null terminator
        let s: Vec<u16> = chars.into_iter().take_while(|&c| c != 0).collect();
        String::from_utf16(&s).ok()
    }
}

struct PtpResponse {
    code: u16,
    data: Option<Vec<u8>>,
}

struct StreamResponse {
    code: u16,
    saw_data: bool,
    bytes_received: u64,
}

impl Drop for PtpIpClient {
    fn drop(&mut self) {
        if self.session_id != 0 {
            let _ = self.close_session();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    #[test]
    fn abort_handle_interrupts_a_blocked_command_read() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted_tx, accepted_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            accepted_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(2));
        });

        let command_stream = Arc::new(TcpStream::connect(address).unwrap());
        command_stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let abort = PtpIpAbortHandle {
            command_stream: Arc::clone(&command_stream),
            event_stream: None,
            cancelled: Arc::clone(&cancelled),
        };
        accepted_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let reader_stream = Arc::clone(&command_stream);
        let blocked_reader = thread::spawn(move || {
            let started = Instant::now();
            let mut byte = [0u8; 1];
            (
                PtpIpClient::read_exact_controlled(&reader_stream, &mut byte, Some(&cancelled)),
                started.elapsed(),
            )
        });

        thread::sleep(Duration::from_millis(30));
        abort.abort();
        let (result, elapsed) = blocked_reader.join().unwrap();
        assert!(
            result.is_err(),
            "socket cancellation must wake the blocked read"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "socket cancellation took too long: {elapsed:?}"
        );

        let _ = release_tx.send(());
        server.join().unwrap();
    }

    #[test]
    fn test_build_packet() {
        let payload = vec![0x01, 0x02, 0x03];
        let pkt = PtpIpClient::build_packet(PTPIP_INIT_COMMAND_REQUEST, &payload);
        assert_eq!(pkt.len(), 11); // 8 header + 3 payload
        let mut cursor = Cursor::new(&pkt);
        let len = cursor.read_u32::<LittleEndian>().unwrap();
        let ptype = cursor.read_u32::<LittleEndian>().unwrap();
        assert_eq!(len, 11);
        assert_eq!(ptype, PTPIP_INIT_COMMAND_REQUEST);
    }

    #[test]
    fn initializes_event_channel_and_uses_zero_for_open_session_transaction() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let responder = thread::spawn(move || {
            let (mut command_stream, _) = listener.accept().unwrap();
            let (packet_type, _) =
                PtpIpClient::read_packet_from(&mut command_stream, "test command").unwrap();
            assert_eq!(packet_type, PTPIP_INIT_COMMAND_REQUEST);

            let connection_id = 0x1234_5678u32;
            command_stream
                .write_all(&PtpIpClient::build_packet(
                    PTPIP_INIT_COMMAND_ACK,
                    &connection_id.to_le_bytes(),
                ))
                .unwrap();

            let (mut event_stream, _) = listener.accept().unwrap();
            let (packet_type, payload) =
                PtpIpClient::read_packet_from(&mut event_stream, "test event").unwrap();
            assert_eq!(packet_type, PTPIP_INIT_EVENT_REQUEST);
            assert_eq!(payload, connection_id.to_le_bytes());
            event_stream
                .write_all(&PtpIpClient::build_packet(PTPIP_INIT_EVENT_ACK, &[]))
                .unwrap();

            let (packet_type, payload) =
                PtpIpClient::read_packet_from(&mut command_stream, "test open session").unwrap();
            let open_session_packet = PtpIpClient::build_packet(packet_type, &payload);
            assert_eq!(
                open_session_packet,
                vec![
                    0x16, 0, 0, 0, 0x06, 0, 0, 0, 0x01, 0, 0, 0, 0x02, 0x10, 0, 0, 0, 0, 0x01, 0,
                    0, 0,
                ]
            );
            let mut response = Vec::new();
            response.write_u16::<LittleEndian>(PTP_OK).unwrap();
            response.write_u32::<LittleEndian>(0).unwrap();
            command_stream
                .write_all(&PtpIpClient::build_packet(
                    PTPIP_COMMAND_RESPONSE,
                    &response,
                ))
                .unwrap();

            let (packet_type, payload) =
                PtpIpClient::read_packet_from(&mut command_stream, "test close session").unwrap();
            let close_session_packet = PtpIpClient::build_packet(packet_type, &payload);
            assert_eq!(
                close_session_packet,
                vec![0x12, 0, 0, 0, 0x06, 0, 0, 0, 0x01, 0, 0, 0, 0x03, 0x10, 0x01, 0, 0, 0,]
            );
            let mut response = Vec::new();
            response.write_u16::<LittleEndian>(PTP_OK).unwrap();
            response.write_u32::<LittleEndian>(1).unwrap();
            command_stream
                .write_all(&PtpIpClient::build_packet(
                    PTPIP_COMMAND_RESPONSE,
                    &response,
                ))
                .unwrap();
        });

        let mut client = PtpIpClient::connect("127.0.0.1", address.port()).unwrap();
        assert!(client.event_stream.is_some());
        assert_eq!(client.open_session().unwrap(), 1);
        assert_eq!(client.transaction_id, 0);
        client.close_session().unwrap();
        assert_eq!(client.transaction_id, 1);
        responder.join().unwrap();
    }

    #[test]
    fn streams_object_packets_without_building_a_full_object_buffer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let responder = thread::spawn(move || {
            let (mut command_stream, _) = listener.accept().unwrap();
            let (packet_type, _) =
                PtpIpClient::read_packet_from(&mut command_stream, "init command").unwrap();
            assert_eq!(packet_type, PTPIP_INIT_COMMAND_REQUEST);
            let connection_id = 77u32;
            command_stream
                .write_all(&PtpIpClient::build_packet(
                    PTPIP_INIT_COMMAND_ACK,
                    &connection_id.to_le_bytes(),
                ))
                .unwrap();

            let (mut event_stream, _) = listener.accept().unwrap();
            let (_, event_payload) =
                PtpIpClient::read_packet_from(&mut event_stream, "init event").unwrap();
            assert_eq!(event_payload, connection_id.to_le_bytes());
            event_stream
                .write_all(&PtpIpClient::build_packet(PTPIP_INIT_EVENT_ACK, &[]))
                .unwrap();

            let (_, open_payload) =
                PtpIpClient::read_packet_from(&mut command_stream, "open session").unwrap();
            assert_eq!(
                u16::from_le_bytes([open_payload[4], open_payload[5]]),
                PTP_OPEN_SESSION
            );
            let mut response = Vec::new();
            response.write_u16::<LittleEndian>(PTP_OK).unwrap();
            response.write_u32::<LittleEndian>(0).unwrap();
            command_stream
                .write_all(&PtpIpClient::build_packet(
                    PTPIP_COMMAND_RESPONSE,
                    &response,
                ))
                .unwrap();

            let (_, object_payload) =
                PtpIpClient::read_packet_from(&mut command_stream, "get object").unwrap();
            assert_eq!(
                u16::from_le_bytes([object_payload[4], object_payload[5]]),
                PTP_GET_OBJECT
            );
            let tid = u32::from_le_bytes(object_payload[6..10].try_into().unwrap());
            assert_eq!(tid, 1);
            assert_eq!(
                u32::from_le_bytes(object_payload[10..14].try_into().unwrap()),
                42
            );

            let original = b"hello world";
            let mut start = Vec::new();
            start.write_u32::<LittleEndian>(tid).unwrap();
            start
                .write_u64::<LittleEndian>(original.len() as u64)
                .unwrap();
            command_stream
                .write_all(&PtpIpClient::build_packet(PTPIP_START_DATA_PACKET, &start))
                .unwrap();

            let mut data = Vec::new();
            data.write_u32::<LittleEndian>(tid).unwrap();
            data.extend_from_slice(b"hello ");
            command_stream
                .write_all(&PtpIpClient::build_packet(PTPIP_DATA_PACKET, &data))
                .unwrap();

            let mut end = Vec::new();
            end.write_u32::<LittleEndian>(tid).unwrap();
            end.extend_from_slice(b"world");
            command_stream
                .write_all(&PtpIpClient::build_packet(PTPIP_END_DATA_PACKET, &end))
                .unwrap();

            let mut response = Vec::new();
            response.write_u16::<LittleEndian>(PTP_OK).unwrap();
            response.write_u32::<LittleEndian>(tid).unwrap();
            command_stream
                .write_all(&PtpIpClient::build_packet(
                    PTPIP_COMMAND_RESPONSE,
                    &response,
                ))
                .unwrap();
        });

        let mut client = PtpIpClient::connect("127.0.0.1", address.port()).unwrap();
        client.open_session().unwrap();
        let mut destination = Vec::new();
        let mut progress = Vec::new();
        let received = client
            .get_object_to_writer(42, &mut destination, |current, total| {
                progress.push((current, total));
            })
            .unwrap();
        client.session_id = 0;

        assert_eq!(received, 11);
        assert_eq!(destination, b"hello world");
        assert_eq!(progress.first(), Some(&(0, 11)));
        assert_eq!(progress.last(), Some(&(11, 11)));
        responder.join().unwrap();
    }

    #[test]
    fn test_photo_format_from() {
        assert_eq!(PhotoFormat::from(0x3801), PhotoFormat::JPEG);
        assert_eq!(PhotoFormat::from(0xB103), PhotoFormat::NEF);
        assert_eq!(PhotoFormat::from(0xB108), PhotoFormat::NEF);
        assert_eq!(PhotoFormat::from(0x380D), PhotoFormat::TIFF);
        assert_eq!(PhotoFormat::from(0x9999), PhotoFormat::Unknown(0x9999));
    }

    #[test]
    fn test_read_utf16_string() {
        // "AB" in UTF-16LE with length prefix 3 (2 chars + null)
        let data = vec![3, b'A', 0, b'B', 0, 0, 0];
        let s = PtpIpClient::read_utf16_string(&data);
        assert_eq!(s, Some("AB".to_string()));
    }

    #[test]
    fn test_read_utf16_string_empty() {
        let data = vec![0];
        assert_eq!(PtpIpClient::read_utf16_string(&data), None);
    }

    #[test]
    fn maps_standard_and_mtp_protection_statuses() {
        assert_eq!(PtpIpClient::map_protection_status(0x0000), (false, true));
        assert_eq!(PtpIpClient::map_protection_status(0x0001), (true, false));
        assert_eq!(PtpIpClient::map_protection_status(0x8001), (true, false));
        assert_eq!(PtpIpClient::map_protection_status(0x8002), (true, false));
        assert_eq!(PtpIpClient::map_protection_status(0x8003), (false, true));
        assert_eq!(PtpIpClient::map_protection_status(0x9000), (true, false));
    }

    #[test]
    fn object_info_exposes_camera_protection() {
        let mut data = Vec::new();
        data.write_u32::<LittleEndian>(1).unwrap(); // storage id
        data.write_u16::<LittleEndian>(0x3801).unwrap(); // JPEG
        data.write_u16::<LittleEndian>(PTP_PROTECTION_READ_ONLY)
            .unwrap();
        data.write_u32::<LittleEndian>(1234).unwrap(); // compressed size
        data.write_u16::<LittleEndian>(0x3801).unwrap(); // thumb format
        data.write_u32::<LittleEndian>(0).unwrap(); // thumb size
        data.write_u32::<LittleEndian>(0).unwrap(); // thumb width
        data.write_u32::<LittleEndian>(0).unwrap(); // thumb height
        data.write_u32::<LittleEndian>(6000).unwrap(); // image width
        data.write_u32::<LittleEndian>(4000).unwrap(); // image height
        data.write_u32::<LittleEndian>(24).unwrap(); // bit depth
        data.write_u32::<LittleEndian>(0).unwrap(); // parent
        data.write_u16::<LittleEndian>(0).unwrap(); // association type
        data.write_u32::<LittleEndian>(0).unwrap(); // association description
        data.write_u32::<LittleEndian>(0).unwrap(); // sequence number
        data.push(2); // one UTF-16 character plus terminator
        data.write_u16::<LittleEndian>('A' as u16).unwrap();
        data.write_u16::<LittleEndian>(0).unwrap();

        let photo = PtpIpClient::parse_object_info(&data, 42).unwrap();
        assert_eq!(photo.filename, "A");
        assert!(photo.protected);
        assert!(!photo.can_delete);
    }
}
