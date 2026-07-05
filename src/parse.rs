use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::protocol::{
    checksum_u16_le, DEFAULT_SCALING_UV, EEG_CHANNELS, MAX_INTERPOLATION_GAP, PACKET_SIZE,
    SYNC_BYTE,
};
use crate::types::{ChecksumStats, EegPacket};

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("invalid packet size: expected {expected}, got {actual}")]
    InvalidSize { expected: usize, actual: usize },
    #[error("invalid sync byte: expected 0xAA, got 0x{0:02X}")]
    InvalidSync(u8),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    InvalidChecksum { expected: u16, actual: u16 },
}

#[derive(Debug, Default)]
pub struct PacketParser {
    pub scaling_uv: f32,
    pub stats: ChecksumStats,
}

impl PacketParser {
    pub fn new(scaling_uv: Option<f32>) -> Self {
        Self {
            scaling_uv: scaling_uv.unwrap_or(DEFAULT_SCALING_UV),
            stats: ChecksumStats::default(),
        }
    }

    pub fn parse_packet(&mut self, packet: &[u8]) -> Result<EegPacket, ParseError> {
        if packet.len() != PACKET_SIZE {
            self.stats.invalid += 1;
            return Err(ParseError::InvalidSize {
                expected: PACKET_SIZE,
                actual: packet.len(),
            });
        }
        if packet[0] != SYNC_BYTE {
            self.stats.invalid += 1;
            return Err(ParseError::InvalidSync(packet[0]));
        }

        let expected = checksum_u16_le(&packet[..PACKET_SIZE - 2]);
        let actual = u16::from_le_bytes([packet[PACKET_SIZE - 2], packet[PACKET_SIZE - 1]]);
        if expected != actual {
            self.stats.invalid += 1;
            return Err(ParseError::InvalidChecksum { expected, actual });
        }
        self.stats.valid += 1;

        let event_id = packet[1];
        let len = packet[2];
        let counter = packet[3];
        let ref_signal = f32::from_le_bytes(packet[4..8].try_into().expect("ref slice"));
        let drl_signal = f32::from_le_bytes(packet[8..12].try_into().expect("drl slice"));

        let mut eeg_uv = [0.0f32; EEG_CHANNELS];
        let mut idx = 12usize;
        for ch in &mut eeg_uv {
            let raw = f32::from_le_bytes(packet[idx..idx + 4].try_into().expect("eeg slice"));
            *ch = raw * self.scaling_uv;
            idx += 4;
        }
        let status = packet[60];
        let checksum = actual;
        let timestamp_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        Ok(EegPacket {
            event_id,
            len,
            counter,
            ref_signal,
            drl_signal,
            eeg_uv,
            status,
            checksum,
            timestamp_us,
        })
    }

    pub fn fill_gaps(previous: &EegPacket, next: &EegPacket) -> Vec<EegPacket> {
        let gap = next.counter.wrapping_sub(previous.counter).wrapping_sub(1);
        if gap == 0 {
            return Vec::new();
        }
        if gap > MAX_INTERPOLATION_GAP {
            return vec![nan_marker(previous.counter.wrapping_add(1), previous, next)];
        }

        (1..=gap)
            .map(|step| {
                let t = step as f32 / (gap + 1) as f32;
                let mut eeg_uv = [0.0f32; EEG_CHANNELS];
                for (idx, out) in eeg_uv.iter_mut().enumerate() {
                    *out = previous.eeg_uv[idx] + t * (next.eeg_uv[idx] - previous.eeg_uv[idx]);
                }
                EegPacket {
                    event_id: previous.event_id,
                    len: previous.len,
                    counter: previous.counter.wrapping_add(step),
                    ref_signal: previous.ref_signal + t * (next.ref_signal - previous.ref_signal),
                    drl_signal: previous.drl_signal + t * (next.drl_signal - previous.drl_signal),
                    eeg_uv,
                    status: previous.status,
                    checksum: 0,
                    timestamp_us: previous.timestamp_us,
                }
            })
            .collect()
    }
}

fn nan_marker(counter: u8, previous: &EegPacket, next: &EegPacket) -> EegPacket {
    EegPacket {
        event_id: previous.event_id,
        len: previous.len,
        counter,
        ref_signal: f32::NAN,
        drl_signal: f32::NAN,
        eeg_uv: [f32::NAN; EEG_CHANNELS],
        status: next.status,
        checksum: 0,
        timestamp_us: previous.timestamp_us,
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::*;
    use crate::protocol::{checksum_u16_le, PACKET_SIZE, SYNC_BYTE};

    fn build_packet(counter: u8, value: f32) -> [u8; PACKET_SIZE] {
        let mut packet = [0u8; PACKET_SIZE];
        packet[0] = SYNC_BYTE;
        packet[1] = 0x10;
        packet[2] = 0x39;
        packet[3] = counter;
        packet[4..8].copy_from_slice(&1.0f32.to_le_bytes());
        packet[8..12].copy_from_slice(&2.0f32.to_le_bytes());
        let mut offset = 12;
        for _ in 0..12 {
            packet[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            offset += 4;
        }
        packet[60] = 0x01;
        let checksum = checksum_u16_le(&packet[..PACKET_SIZE - 2]);
        packet[61..63].copy_from_slice(&checksum.to_le_bytes());
        packet
    }

    #[test]
    fn parses_valid_packet() {
        let mut parser = PacketParser::new(None);
        let pkt = parser.parse_packet(&build_packet(7, 10.0)).unwrap();
        assert_eq!(pkt.counter, 7);
        assert_relative_eq!(pkt.eeg_uv[0], 10.0 * DEFAULT_SCALING_UV);
        assert_eq!(parser.stats.valid, 1);
    }

    #[test]
    fn rejects_bad_checksum() {
        let mut parser = PacketParser::new(None);
        let mut packet = build_packet(1, 1.0);
        packet[10] ^= 0xFF;
        let err = parser.parse_packet(&packet).unwrap_err();
        assert!(matches!(err, ParseError::InvalidChecksum { .. }));
        assert_eq!(parser.stats.invalid, 1);
    }

    #[test]
    fn gap_fill_interpolates_small_gaps() {
        let previous = EegPacket {
            event_id: 0,
            len: 0,
            counter: 10,
            ref_signal: 0.0,
            drl_signal: 0.0,
            eeg_uv: [0.0; 12],
            status: 0,
            checksum: 0,
            timestamp_us: 1,
        };
        let next = EegPacket {
            counter: 12,
            eeg_uv: [2.0; 12],
            ..previous.clone()
        };

        let inserted = PacketParser::fill_gaps(&previous, &next);
        assert_eq!(inserted.len(), 1);
        assert_eq!(inserted[0].counter, 11);
        assert_relative_eq!(inserted[0].eeg_uv[0], 1.0);
    }

    #[test]
    fn gap_fill_marks_large_gaps_nan() {
        let previous = EegPacket {
            event_id: 0,
            len: 0,
            counter: 10,
            ref_signal: 0.0,
            drl_signal: 0.0,
            eeg_uv: [0.0; 12],
            status: 0,
            checksum: 0,
            timestamp_us: 1,
        };
        let next = EegPacket {
            counter: 20,
            eeg_uv: [2.0; 12],
            ..previous.clone()
        };
        let inserted = PacketParser::fill_gaps(&previous, &next);
        assert_eq!(inserted.len(), 1);
        assert!(inserted[0].eeg_uv[0].is_nan());
    }
}
