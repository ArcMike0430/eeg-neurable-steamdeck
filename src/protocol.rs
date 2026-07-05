//! MW75 Neuro protocol constants — UUIDs, GAIA commands, packet layout.
//!
//! All UUIDs and command bytes are confirmed working against the MW75 firmware
//! (validated on macOS/iOS reference implementation). USB-BT500 with the
//! btusb/RTL8761B driver gives identical behaviour on Linux.

// ---- Vendor-specific Bluetooth UUIDs ----------------------------------------

/// Primary GAIA service UUID (MW75 vendor-specific).
pub const GAIA_SERVICE_UUID: &str = "00001100-d102-11e1-9b23-00025b00a5a5";

/// GAIA control characteristic — write GAIA commands here.
pub const GAIA_CONTROL_UUID: &str = "00001101-d102-11e1-9b23-00025b00a5a5";

/// EEG data service UUID (raw 12-channel stream).
pub const EEG_SERVICE_UUID: &str = "df21fe2c-2515-4fdb-8886-f12c4d679277";

// ---- 5-byte GAIA command payloads -------------------------------------------
//
// Format: [vendor_id_hi, vendor_id_lo, msg_type, cmd, param]

/// Tell the headphones to power on the EEG front-end.
pub const ENABLE_EEG_CMD: [u8; 5] = [0x02, 0x01, 0x00, 0x01, 0x00];

/// Enable raw (unfiltered) 500 Hz acquisition mode.
pub const ENABLE_RAW_MODE_CMD: [u8; 5] = [0x02, 0x01, 0x00, 0x02, 0x01];

/// Query battery level.
pub const BATTERY_CMD: [u8; 5] = [0x02, 0x02, 0x00, 0x00, 0x00];

// ---- Timing (milliseconds) --------------------------------------------------

/// Delay after ENABLE_EEG before sending ENABLE_RAW_MODE.
pub const GAIA_ENABLE_DELAY_MS: u64 = 100;

/// Delay after ENABLE_RAW_MODE before RFCOMM starts delivering frames.
pub const GAIA_RAW_MODE_DELAY_MS: u64 = 500;

// ---- RFCOMM -----------------------------------------------------------------

/// RFCOMM channel used by MW75 for EEG data streaming.
pub const RFCOMM_EEG_CHANNEL: u8 = 25;

/// RFCOMM channel used by MW75 for heartbeat / keep-alive.
pub const RFCOMM_HEARTBEAT_CHANNEL: u8 = 1;

/// Expected heartbeat interval from the device.
pub const HEARTBEAT_INTERVAL_SECS: u64 = 1;

/// If no heartbeat is received within this window, flag a timeout.
pub const HEARTBEAT_TIMEOUT_SECS: u64 = 5;

/// GAIA session expires ~60 s after the last GAIA write without a keep-alive.
pub const GAIA_SESSION_TIMEOUT_SECS: u64 = 60;

/// Maximum reconnect backoff (exponential: 1 s → 2 s → 4 s → … → 30 s).
pub const RECONNECT_BACKOFF_MAX_SECS: u64 = 30;

// ---- EEG frame layout (little-endian) ---------------------------------------
//
// The MW75 sends 63-byte frames over RFCOMM ch25.
//
// Byte  | Field       | Type    | Description
// ------|-------------|---------|--------------------------------------
//  0-2  | SOF         | [u8;3]  | Start-of-frame marker (0xA0, 0xA0, counter_lo)
//  3    | counter     | u8      | Rolling frame counter
//  4-7  | REF         | f32 LE  | Reference electrode voltage
//  8-11 | DRL         | f32 LE  | Driven-right-leg electrode voltage
// 12-59 | EEG[0..11]  | f32 LE  | 12 × 4 bytes EEG channel voltages
// 60    | status      | u8      | Device status flags
// 61-62 | checksum    | u16 LE  | 16-bit little-endian checksum

/// Total frame size in bytes.
pub const FRAME_SIZE: usize = 63;

/// Start-of-frame marker (first two bytes must equal SOF_BYTE).
pub const SOF_BYTE: u8 = 0xA0;

/// Byte offset: frame counter (u8).
pub const OFF_COUNTER: usize = 3;

/// Byte offset: REF electrode (f32 LE).
pub const OFF_REF: usize = 4;

/// Byte offset: DRL electrode (f32 LE).
pub const OFF_DRL: usize = 8;

/// Byte offset: first EEG channel (f32 LE); channels are packed contiguously.
pub const OFF_EEG: usize = 12;

/// Byte offset: device status byte.
pub const OFF_STATUS: usize = 60;

/// Byte offset: 16-bit little-endian checksum.
pub const OFF_CHECKSUM: usize = 61;

/// Number of EEG channels.
pub const EEG_CHANNEL_COUNT: usize = 12;

/// Nominal sample rate in Hz.
pub const EEG_SAMPLE_RATE: u32 = 500;

/// Maximum acceptable gap (frames) before we give up on interpolation.
pub const MAX_INTERP_GAP: u8 = 3;

// ---- Ring-buffer / backpressure tuning --------------------------------------

/// Number of frames to hold in the RFCOMM ring buffer (≈2 s at 500 Hz).
pub const RING_BUFFER_FRAMES: usize = 1000;

/// If no frame arrives within this many milliseconds, log a stall warning.
pub const STALL_THRESHOLD_MS: u64 = 50;

// ---- Peer sync --------------------------------------------------------------

/// UDP multicast address for peer epoch broadcasts.
pub const SYNC_MULTICAST_ADDR: &str = "224.0.0.1";

/// UDP port for peer epoch broadcasts.
pub const SYNC_PORT: u16 = 5005;

/// Epoch length in seconds (one broadcast per epoch).
pub const SYNC_EPOCH_SECS: u64 = 1;
