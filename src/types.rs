//! Core data types for the EEG acquisition pipeline.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::protocol::{EEG_CHANNEL_COUNT, SAMPLE_RATE_HZ};

// ── Raw EEG Packet ───────────────────────────────────────────────────────────

/// A single 63-byte raw EEG frame decoded from the RFCOMM stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EegPacket {
    /// Wall-clock timestamp when the packet was received.
    pub timestamp: DateTime<Utc>,

    /// Rolling counter (0–255) for packet-loss detection.
    pub counter: u8,

    /// Reference electrode value in µV.
    pub ref_uv: f32,

    /// DRL (Driven Right Leg) electrode value in µV.
    pub drl_uv: f32,

    /// EEG channel voltages in µV (12 channels, indices 0–11).
    pub channels: [f32; EEG_CHANNEL_COUNT],

    /// Device status byte (signal quality flags etc.).
    pub status: u8,

    /// Raw checksum bytes from the packet (validation already done).
    pub checksum: u16,
}

impl EegPacket {
    /// Returns whether all channels appear to have valid signal quality.
    pub fn is_good_signal(&self) -> bool {
        // MW75 uses 0x00 in status byte for good signal
        self.status == 0x00
    }
}

// ── Events ───────────────────────────────────────────────────────────────────

/// Top-level events produced by the MW75 client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Mw75Event {
    /// A decoded EEG data frame.
    Eeg(EegPacket),

    /// Battery state update.
    Battery(BatteryInfo),

    /// Device connected.
    Connected { device_name: String, address: String },

    /// Device disconnected.
    Disconnected { address: String },

    /// RFCOMM streaming started.
    StreamStarted,

    /// RFCOMM streaming stopped.
    StreamStopped,

    /// A protocol error (bad checksum, unexpected SOF, etc.).
    Error(ProtocolError),
}

// ── Battery ──────────────────────────────────────────────────────────────────

/// Battery state reported by the headset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryInfo {
    /// Battery level 0–100 %.
    pub level_pct: u8,

    /// Whether the headset is currently charging.
    pub is_charging: bool,

    /// Time of the reading.
    pub timestamp: DateTime<Utc>,
}

// ── Protocol Errors ──────────────────────────────────────────────────────────

/// Errors that can occur while parsing or validating EEG packets.
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum ProtocolError {
    #[error("bad start-of-frame: expected 0xAA, got {got:#04x}")]
    BadSof { got: u8 },

    #[error("packet too short: expected {expected} bytes, got {got}")]
    PacketTooShort { expected: usize, got: usize },

    #[error("checksum mismatch: expected {expected:#06x}, calculated {calculated:#06x}")]
    ChecksumMismatch { expected: u16, calculated: u16 },

    #[error("unexpected event id: {0:#04x}")]
    UnknownEventId(u8),
}

// ── Streaming Config ─────────────────────────────────────────────────────────

/// Runtime configuration for the EEG streaming session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Target Bluetooth address of the MW75 (e.g. `"AA:BB:CC:DD:EE:FF"`).
    pub device_address: Option<String>,

    /// Override RFCOMM channel (default: 25).
    pub rfcomm_channel: u8,

    /// Enable simulation mode (no real hardware needed).
    pub simulate: bool,

    /// Sample rate in Hz (default: 500).
    pub sample_rate_hz: u32,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            device_address: None,
            rfcomm_channel: crate::protocol::RFCOMM_CHANNEL,
            simulate: false,
            sample_rate_hz: SAMPLE_RATE_HZ,
        }
    }
}

// ── Ring-buffer Frame ────────────────────────────────────────────────────────

/// A timestamped multi-channel EEG sample for ring-buffer storage.
#[derive(Debug, Clone, Copy)]
pub struct EegSample {
    pub timestamp_us: i64,
    pub channels: [f32; EEG_CHANNEL_COUNT],
}
