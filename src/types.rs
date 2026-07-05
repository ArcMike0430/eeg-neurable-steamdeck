use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EegPacket {
    pub event_id: u8,
    pub len: u8,
    pub counter: u8,
    pub ref_signal: f32,
    pub drl_signal: f32,
    pub eeg_uv: [f32; 12],
    pub status: u8,
    pub checksum: u16,
    pub timestamp_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Mw75Event {
    Eeg(EegPacket),
    Battery(BatteryInfo),
    Stall,
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatteryInfo {
    pub percent: Option<u8>,
    pub firmware_version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChecksumStats {
    pub valid: u64,
    pub invalid: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerSyncMessage {
    pub device_id: String,
    pub epoch_num: u64,
    pub timestamp_us: u64,
    pub checksum: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdapterInfo {
    pub name: String,
    pub address: Option<String>,
    pub driver: Option<String>,
    pub is_usb: bool,
}
