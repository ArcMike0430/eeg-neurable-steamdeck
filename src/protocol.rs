use std::time::Duration;

pub const GAIA_CONTROL_UUID: &str = "00001101-d102-11e1-9b23-00025b00a5a5";
pub const GAIA_SERVICE_UUID: &str = "00001100-d102-11e1-9b23-00025b00a5a5";
pub const EEG_SERVICE_UUID: &str = "df21fe2c-2515-4fdb-8886-f12c4d679277";

pub const RFCOMM_CHANNEL_EEG: u8 = 25;
pub const RFCOMM_CHANNEL_HEARTBEAT: u8 = 1;

pub const PACKET_SIZE: usize = 63;
pub const EEG_CHANNELS: usize = 12;
pub const SYNC_BYTE: u8 = 0xAA;
pub const MAX_INTERPOLATION_GAP: u8 = 3;
pub const STALL_THRESHOLD: Duration = Duration::from_millis(50);

pub const DEFAULT_SCALING_UV: f32 = 0.023842;

pub const ENABLE_EEG_CMD: [u8; 5] = [0x02, 0x01, 0x00, 0x01, 0x00];
pub const ENABLE_RAW_MODE_CMD: [u8; 5] = [0x02, 0x01, 0x00, 0x02, 0x01];
pub const BATTERY_CMD: [u8; 5] = [0x02, 0x02, 0x00, 0x00, 0x00];

pub const GAIA_ACTIVATION_SEQUENCE: [([u8; 5], Duration); 3] = [
    (ENABLE_EEG_CMD, Duration::from_millis(100)),
    (ENABLE_RAW_MODE_CMD, Duration::from_millis(500)),
    (BATTERY_CMD, Duration::from_millis(0)),
];

pub fn checksum_u16_le(packet_wo_checksum: &[u8]) -> u16 {
    packet_wo_checksum
        .iter()
        .fold(0u16, |acc, byte| acc.wrapping_add(u16::from(*byte)))
}
