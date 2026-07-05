//! GATT UUIDs, BLE command bytes, and protocol constants for Neurable MW75.

use uuid::Uuid;

// ── GATT Service & Characteristic UUIDs ─────────────────────────────────────

/// Primary service UUID for MW75 Neuro EEG
pub const SERVICE_UUID: Uuid = Uuid::from_u128(0x0000fe59_0000_1000_8000_00805f9b34fb);

/// Characteristic for sending control commands (Write Without Response)
pub const CMD_CHAR_UUID: Uuid = Uuid::from_u128(0x0000fe5a_0000_1000_8000_00805f9b34fb);

/// Characteristic for receiving BLE notifications (Notify)
pub const NOTIFY_CHAR_UUID: Uuid = Uuid::from_u128(0x0000fe5b_0000_1000_8000_00805f9b34fb);

// ── BLE Activation Command Bytes ─────────────────────────────────────────────

/// Enable EEG streaming command
pub const ENABLE_EEG: &[u8] = &[0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03];

/// Enable raw ADC mode (500 Hz, 12-channel)
pub const ENABLE_RAW_MODE: &[u8] = &[0x02, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03];

/// Request battery status
pub const BATTERY_CMD: &[u8] = &[0x02, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03];

/// Disable EEG streaming command
pub const DISABLE_EEG: &[u8] = &[0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03];

// ── BLE Activation Timing ────────────────────────────────────────────────────

/// Delay after ENABLE_EEG before sending ENABLE_RAW_MODE (ms)
pub const EEG_ENABLE_DELAY_MS: u64 = 100;

/// Delay after ENABLE_RAW_MODE before sending BATTERY_CMD (ms)
pub const RAW_MODE_DELAY_MS: u64 = 500;

// ── RFCOMM Transport ─────────────────────────────────────────────────────────

/// RFCOMM channel for MW75 raw EEG streaming
pub const RFCOMM_CHANNEL: u8 = 25;

/// Fixed packet size (bytes) for raw EEG frames
pub const PACKET_SIZE: usize = 63;

/// Packet start-of-frame marker
pub const PACKET_SOF: u8 = 0xAA;

/// Number of EEG channels
pub const EEG_CHANNEL_COUNT: usize = 12;

/// ADC to µV scaling factor
pub const ADC_UV_SCALE: f32 = 0.023842;

/// Nominal EEG sample rate (Hz) in raw mode
pub const SAMPLE_RATE_HZ: u32 = 500;

// ── Packet Layout Offsets ────────────────────────────────────────────────────

/// Offset of the start-of-frame byte
pub const OFF_SOF: usize = 0;

/// Offset of the event ID byte
pub const OFF_EVENT_ID: usize = 1;

/// Offset of the payload length byte
pub const OFF_LEN: usize = 2;

/// Offset of the packet counter byte
pub const OFF_COUNTER: usize = 3;

/// Offset of the reference electrode f32 (big-endian)
pub const OFF_REF: usize = 4;

/// Offset of the DRL electrode f32 (big-endian)
pub const OFF_DRL: usize = 8;

/// Offset of the first EEG channel f32 (12 × 4 bytes = 48 bytes)
pub const OFF_EEG_START: usize = 12;

/// Offset of the status byte (after 12 channels)
pub const OFF_STATUS: usize = 60;

/// Offset of the checksum (u16 LE)
pub const OFF_CHECKSUM: usize = 61;

// ── Event IDs ────────────────────────────────────────────────────────────────

/// Event ID for raw EEG frames
pub const EVT_EEG: u8 = 0x01;

/// Event ID for battery notification
pub const EVT_BATTERY: u8 = 0x09;

/// Event ID for device info / firmware version
pub const EVT_INFO: u8 = 0x05;

/// Expected device name prefix for MW75 discovery
pub const DEVICE_NAME_PREFIX: &str = "MW75";
