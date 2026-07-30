//! PTP/IP protocol implementation for Nikon cameras.
//! Ported from NikonPtpIpClient.java (nikon-image-transmission-main).
//!
//! Protocol flow:
//! 1. Init_Command_Request (1) -> Init_Command_Ack (2)
//! 2. Open_Session (0x1002)
//! 3. Get_Object_Handles (0x1007)
//! 4. Get_Object_Info (0x1008) / Get_Thumb (0x100A) / Get_Object (0x1009)

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{self, Cursor, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;
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
const PTPIP_COMMAND_REQUEST: u32 = 6;
const PTPIP_COMMAND_RESPONSE: u32 = 7;
const PTPIP_START_DATA_PACKET: u32 = 9;
const PTPIP_DATA_PACKET: u32 = 10;
const PTPIP_END_DATA_PACKET: u32 = 12;

// PTP operation codes
const PTP_OPEN_SESSION: u16 = 0x1002;
const PTP_CLOSE_SESSION: u16 = 0x1003;
const PTP_GET_DEVICE_INFO: u16 = 0x1001;
const PTP_GET_OBJECT_HANDLES: u16 = 0x1007;
const PTP_GET_OBJECT_INFO: u16 = 0x1008;
const PTP_GET_OBJECT: u16 = 0x1009;
const PTP_GET_THUMB: u16 = 0x100A;

// PTP response codes
const PTP_OK: u16 = 0x2001;
const PTP_SESSION_ALREADY_OPEN: u16 = 0x201E;

/// Default Nikon AP mode address
pub const DEFAULT_AP_HOST: &str = "192.168.1.1";
pub const DEFAULT_PTPIP_PORT: u16 = 15740;

pub struct PtpIpClient {
    cmd_stream: TcpStream,
    data_stream: Option<TcpStream>,
    session_id: u32,
    transaction_id: u32,
    guid: [u8; 16],
}

impl PtpIpClient {
    /// Connect to camera via PTP/IP
    pub fn connect(host: &str, port: u16) -> Result<Self, PtpError> {
        let addr: SocketAddr = format!("{}:{}", host, port)
            .parse()
            .map_err(|e| PtpError::Connection(format!("Invalid address: {}", e)))?;

        let cmd_stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
            .map_err(|e| PtpError::Connection(format!("Failed to connect: {}", e)))?;
        cmd_stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        cmd_stream.set_write_timeout(Some(Duration::from_secs(10)))?;

        let mut client = Self {
            cmd_stream,
            data_stream: None,
            session_id: 0,
            transaction_id: 0,
            guid: [0u8; 16],
        };

        client.init_handshake()?;
        Ok(client)
    }

    /// Perform PTP/IP init handshake
    fn init_handshake(&mut self) -> Result<(), PtpError> {
        // Generate a random GUID
        for (i, b) in self.guid.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(17).wrapping_add(42);
        }

        // Build Init_Command_Request packet
        let mut payload = Vec::new();
        payload.extend_from_slice(&self.guid);
        // Host name (UTF-16LE, null-terminated): "NikonPhotoStudio"
        let name = "NikonPhotoStudio";
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

        // Parse ack: connection_id (4 bytes) + guid (16 bytes) + name + version
        if data.len() < 4 {
            return Err(PtpError::Protocol("Init ack too short".into()));
        }

        let connection_id = Cursor::new(&data[..4]).read_u32::<LittleEndian>()?;
        // Open the required event connection.
        let peer = self.cmd_stream.peer_addr()?;
        let data_stream = TcpStream::connect_timeout(&peer, Duration::from_secs(10))
            .map_err(|e| PtpError::Connection(format!("Data channel failed: {}", e)))?;
        data_stream.set_read_timeout(Some(Duration::from_secs(60)))?;

        // Associate the event socket with the command/data socket.
        let mut data_payload = Vec::new();
        data_payload.write_u32::<LittleEndian>(connection_id)?;

        let mut ds = &data_stream;
        let pkt = Self::build_packet(PTPIP_INIT_EVENT_REQUEST, &data_payload);
        ds.write_all(&pkt)?;
        let mut header = [0u8; 8];
        ds.read_exact(&mut header)?;
        let mut event_header = Cursor::new(&header);
        let event_len = event_header.read_u32::<LittleEndian>()?;
        let event_type = event_header.read_u32::<LittleEndian>()?;
        if event_type != PTPIP_INIT_EVENT_ACK || event_len < 8 {
            return Err(PtpError::Protocol(format!("Expected Init_Event_Ack (4), got {event_type}")));
        }
        if event_len > 8 {
            let mut discard = vec![0u8; (event_len - 8) as usize];
            ds.read_exact(&mut discard)?;
        }

        self.data_stream = Some(data_stream);
        Ok(())
    }

    /// Open a PTP session
    pub fn open_session(&mut self) -> Result<u32, PtpError> {
        self.transaction_id += 1;
        let tid = self.transaction_id;

        let mut params = Vec::new();
        params.write_u32::<LittleEndian>(1)?; // session ID = 1

        let resp = self.send_command(PTP_OPEN_SESSION, tid, &params)?;
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
        if resp.code != PTP_OK { return Err(PtpError::Protocol(format!("GetDeviceInfo failed: 0x{:04X}", resp.code))); }
        let data = resp.data.ok_or_else(|| PtpError::Protocol("GetDeviceInfo returned no data".into()))?;
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
        Ok(if model.to_ascii_lowercase().contains("nikon") { model } else { format!("{manufacturer} {model}") })
    }

    fn read_ptp_string(cursor: &mut Cursor<&[u8]>) -> Result<Option<String>, PtpError> {
        let len = cursor.read_u8()? as usize;
        if len == 0 { return Ok(None); }
        let mut chars = Vec::with_capacity(len);
        for _ in 0..len { chars.push(cursor.read_u16::<LittleEndian>()?); }
        while chars.last() == Some(&0) { chars.pop(); }
        Ok(String::from_utf16(&chars).ok())
    }

    /// Get all object handles from the camera
    pub fn get_object_handles(&mut self) -> Result<Vec<u32>, PtpError> {
        self.transaction_id += 1;
        let tid = self.transaction_id;

        let mut params = Vec::new();
        params.write_u32::<LittleEndian>(0xFFFFFFFF)?; // storage: all
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
        let data = resp.data.ok_or_else(|| PtpError::Protocol("No data phase".into()))?;
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

        let data = resp.data.ok_or_else(|| PtpError::Protocol("No data".into()))?;
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

        resp.data.ok_or_else(|| PtpError::Protocol("No object data".into()))
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

        resp.data.ok_or_else(|| PtpError::Protocol("No thumb data".into()))
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

    fn send_command(&mut self, opcode: u16, tid: u32, params: &[u8]) -> Result<PtpResponse, PtpError> {
        // Build Command_Request packet
        let mut payload = Vec::new();
        // CIPA PTP/IP: 1=no data phase, 2=data out, 3=data in. All read
        // operations used by the browser require data-in; session commands do
        // not. Advertising "no data" for GetObject was why a connected Nikon
        // returned an empty grid in the former implementation.
        let data_phase = match opcode {
            PTP_GET_DEVICE_INFO | PTP_GET_OBJECT_HANDLES | PTP_GET_OBJECT_INFO | PTP_GET_OBJECT | PTP_GET_THUMB => 3,
            _ => 1,
        };
        payload.write_u32::<LittleEndian>(data_phase)?;
        payload.write_u16::<LittleEndian>(opcode)?;
        payload.write_u32::<LittleEndian>(tid)?;
        payload.extend_from_slice(params);

        self.write_packet(PTPIP_COMMAND_REQUEST, &payload)?;

        // Read response (may include data packets)
        self.read_response()
    }

    fn read_response(&mut self) -> Result<PtpResponse, PtpError> {
        let mut data_buf: Option<Vec<u8>> = None;
        let mut response_code = 0u16;

        loop {
            let (ptype, data) = self.read_packet()?;
            match ptype {
                PTPIP_START_DATA_PACKET => {
                    // transaction id (4) + total length (8) + optional data
                    let mut buf = Vec::new();
                    if data.len() > 12 {
                        buf.extend_from_slice(&data[12..]);
                    }
                    data_buf = Some(buf);
                }
                PTPIP_DATA_PACKET => {
                    if let Some(ref mut buf) = data_buf {
                        if data.len() > 4 { buf.extend_from_slice(&data[4..]); }
                    }
                }
                PTPIP_END_DATA_PACKET => {
                    if let Some(ref mut buf) = data_buf {
                        if data.len() > 4 { buf.extend_from_slice(&data[4..]); }
                    }
                }
                PTPIP_COMMAND_RESPONSE => {
                    // Parse: response_code(2) + transaction_id(4) + params
                    let mut cursor = Cursor::new(&data);
                    response_code = cursor.read_u16::<LittleEndian>()?;
                    let _tid = cursor.read_u32::<LittleEndian>()?;
                    break;
                }
                other => {
                    return Err(PtpError::Protocol(format!("Unexpected packet type: {}", other)));
                }
            }
        }

        Ok(PtpResponse { code: response_code, data: data_buf })
    }

    fn write_packet(&mut self, ptype: u32, payload: &[u8]) -> Result<(), PtpError> {
        let pkt = Self::build_packet(ptype, payload);
        self.cmd_stream.write_all(&pkt)?;
        self.cmd_stream.flush()?;
        Ok(())
    }

    fn read_packet(&mut self) -> Result<(u32, Vec<u8>), PtpError> {
        // Read 8-byte header: length(4) + type(4)
        let mut header = [0u8; 8];
        self.cmd_stream.read_exact(&mut header)?;
        let mut cursor = Cursor::new(&header);
        let length = cursor.read_u32::<LittleEndian>()?;
        let ptype = cursor.read_u32::<LittleEndian>()?;

        if length < 8 {
            return Err(PtpError::Protocol("Invalid packet length".into()));
        }

        let payload_len = (length - 8) as usize;
        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 {
            self.cmd_stream.read_exact(&mut payload)?;
        }

        Ok((ptype, payload))
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
        let _protection = cursor.read_u16::<LittleEndian>()?;
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
        let filename = Self::read_utf16_string(remaining).unwrap_or_else(|| format!("OBJ_{:08X}", handle));

        Ok(CameraPhoto {
            handle,
            filename,
            size: compressed_size as u64,
            format: PhotoFormat::from(format_code),
            date: String::new(),
            width,
            height,
            source_path: None,
        })
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
}
