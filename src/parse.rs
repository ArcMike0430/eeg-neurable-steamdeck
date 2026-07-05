//! Packet parsing, checksum validation, and frame synchronisation.

use byteorder::{BigEndian, ReadBytesExt};
use chrono::Utc;
use std::io::Cursor;

use crate::{
    protocol::*,
    types::{EegPacket, ProtocolError},
};

// ── Public interface ─────────────────────────────────────────────────────────

/// Attempt to parse a 63-byte EEG packet from a raw byte slice.
///
/// Returns `Ok(EegPacket)` if the packet is well-formed and the checksum
/// validates, or `Err(ProtocolError)` on any violation.
pub fn parse_eeg_packet(buf: &[u8]) -> Result<EegPacket, ProtocolError> {
    if buf.len() < PACKET_SIZE {
        return Err(ProtocolError::PacketTooShort {
            expected: PACKET_SIZE,
            got: buf.len(),
        });
    }

    if buf[OFF_SOF] != PACKET_SOF {
        return Err(ProtocolError::BadSof { got: buf[OFF_SOF] });
    }

    let event_id = buf[OFF_EVENT_ID];
    if event_id != EVT_EEG {
        return Err(ProtocolError::UnknownEventId(event_id));
    }

    // Validate checksum (sum of bytes 0..60, u16 LE, stored at bytes 61-62)
    let expected_checksum = checksum(&buf[..OFF_CHECKSUM]);
    let stored_checksum = u16::from_le_bytes([buf[OFF_CHECKSUM], buf[OFF_CHECKSUM + 1]]);
    if expected_checksum != stored_checksum {
        return Err(ProtocolError::ChecksumMismatch {
            expected: stored_checksum,
            calculated: expected_checksum,
        });
    }

    let counter = buf[OFF_COUNTER];
    let status = buf[OFF_STATUS];

    let ref_uv = read_f32_be(&buf[OFF_REF..OFF_REF + 4]) * ADC_UV_SCALE;
    let drl_uv = read_f32_be(&buf[OFF_DRL..OFF_DRL + 4]) * ADC_UV_SCALE;

    let mut channels = [0f32; EEG_CHANNEL_COUNT];
    for (i, ch) in channels.iter_mut().enumerate() {
        let off = OFF_EEG_START + i * 4;
        *ch = read_f32_be(&buf[off..off + 4]) * ADC_UV_SCALE;
    }

    Ok(EegPacket {
        timestamp: Utc::now(),
        counter,
        ref_uv,
        drl_uv,
        channels,
        status,
        checksum: stored_checksum,
    })
}

/// Parse a battery notification payload (2 bytes: level, flags).
pub fn parse_battery(payload: &[u8]) -> Option<(u8, bool)> {
    if payload.len() < 2 {
        return None;
    }
    let level = payload[0].min(100);
    let charging = payload[1] & 0x01 != 0;
    Some((level, charging))
}

// ── Frame synchroniser ───────────────────────────────────────────────────────

/// Maintains a sliding window over a raw byte stream to align on 0xAA SOF
/// bytes and extract complete 63-byte frames.
#[derive(Default)]
pub struct FrameSync {
    buf: Vec<u8>,
}

impl FrameSync {
    pub fn new() -> Self {
        Self { buf: Vec::with_capacity(PACKET_SIZE * 4) }
    }

    /// Feed raw bytes into the synchroniser.  Fully-parsed packets are
    /// appended to `out`; parse errors are collected in `errors`.
    pub fn feed(
        &mut self,
        data: &[u8],
        out: &mut Vec<EegPacket>,
        errors: &mut Vec<ProtocolError>,
    ) {
        self.buf.extend_from_slice(data);

        loop {
            // Find next SOF marker
            let start = match self.buf.iter().position(|&b| b == PACKET_SOF) {
                Some(p) => p,
                None => {
                    self.buf.clear();
                    break;
                }
            };

            // Discard bytes before the SOF
            if start > 0 {
                self.buf.drain(..start);
            }

            if self.buf.len() < PACKET_SIZE {
                break; // wait for more data
            }

            let frame = self.buf[..PACKET_SIZE].to_vec();
            self.buf.drain(..PACKET_SIZE);

            match parse_eeg_packet(&frame) {
                Ok(pkt) => out.push(pkt),
                Err(e) => {
                    errors.push(e);
                    // Re-sync: skip the leading SOF and retry
                    if !self.buf.is_empty() {
                        self.buf.drain(..1);
                    }
                }
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Read a big-endian f32 from a 4-byte slice.
fn read_f32_be(bytes: &[u8]) -> f32 {
    let mut cur = Cursor::new(bytes);
    cur.read_f32::<BigEndian>().unwrap_or(0.0)
}

/// Compute the packet checksum: sum of all payload bytes as a u16 (wrapping).
pub fn checksum(data: &[u8]) -> u16 {
    data.iter().fold(0u16, |acc, &b| acc.wrapping_add(b as u16))
}

/// Build a synthetic 63-byte EEG packet from structured data (for simulation /
/// test fixtures).
pub fn build_packet(counter: u8, channels: &[f32; EEG_CHANNEL_COUNT], status: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[OFF_SOF] = PACKET_SOF;
    buf[OFF_EVENT_ID] = EVT_EEG;
    buf[OFF_LEN] = 0x38; // 56 payload bytes
    buf[OFF_COUNTER] = counter;

    // REF and DRL both 0.0
    buf[OFF_REF..OFF_REF + 4].copy_from_slice(&0f32.to_be_bytes());
    buf[OFF_DRL..OFF_DRL + 4].copy_from_slice(&0f32.to_be_bytes());

    for (i, &ch) in channels.iter().enumerate() {
        let raw = ch / ADC_UV_SCALE;
        let off = OFF_EEG_START + i * 4;
        buf[off..off + 4].copy_from_slice(&raw.to_be_bytes());
    }

    buf[OFF_STATUS] = status;

    let cs = checksum(&buf[..OFF_CHECKSUM]);
    buf[OFF_CHECKSUM] = (cs & 0xFF) as u8;
    buf[OFF_CHECKSUM + 1] = (cs >> 8) as u8;

    buf
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_channels(v: f32) -> [f32; EEG_CHANNEL_COUNT] {
        [v; EEG_CHANNEL_COUNT]
    }

    #[test]
    fn roundtrip_packet() {
        let channels = make_channels(10.0);
        let raw = build_packet(42, &channels, 0x00);
        let pkt = parse_eeg_packet(&raw).expect("parse should succeed");
        assert_eq!(pkt.counter, 42);
        assert_eq!(pkt.status, 0x00);
        for ch in &pkt.channels {
            // Floating-point round-trip: tolerance ~0.001 µV
            assert!((ch - 10.0).abs() < 0.01, "channel value {ch} != 10.0");
        }
    }

    #[test]
    fn bad_sof_detected() {
        let channels = make_channels(0.0);
        let mut raw = build_packet(0, &channels, 0);
        raw[0] = 0x00; // corrupt SOF
        // Recompute checksum to keep that test clean
        let cs = checksum(&raw[..OFF_CHECKSUM]);
        raw[OFF_CHECKSUM] = (cs & 0xFF) as u8;
        raw[OFF_CHECKSUM + 1] = (cs >> 8) as u8;
        let err = parse_eeg_packet(&raw).unwrap_err();
        assert!(matches!(err, ProtocolError::BadSof { .. }));
    }

    #[test]
    fn checksum_mismatch_detected() {
        let channels = make_channels(5.0);
        let mut raw = build_packet(1, &channels, 0);
        raw[OFF_CHECKSUM] ^= 0xFF; // corrupt checksum
        let err = parse_eeg_packet(&raw).unwrap_err();
        assert!(matches!(err, ProtocolError::ChecksumMismatch { .. }));
    }

    #[test]
    fn framesync_extracts_multiple_packets() {
        let ch = make_channels(1.0);
        let p1 = build_packet(0, &ch, 0);
        let p2 = build_packet(1, &ch, 0);
        let mut combined = p1.to_vec();
        combined.extend_from_slice(&p2);

        let mut sync = FrameSync::new();
        let mut out = Vec::new();
        let mut errs = Vec::new();
        sync.feed(&combined, &mut out, &mut errs);

        assert_eq!(out.len(), 2);
        assert!(errs.is_empty());
        assert_eq!(out[0].counter, 0);
        assert_eq!(out[1].counter, 1);
    }

    #[test]
    fn framesync_handles_leading_garbage() {
        let ch = make_channels(0.0);
        let pkt = build_packet(7, &ch, 0);
        let mut data = vec![0xDE, 0xAD, 0xBE, 0xEF]; // garbage
        data.extend_from_slice(&pkt);

        let mut sync = FrameSync::new();
        let mut out = Vec::new();
        let mut errs = Vec::new();
        sync.feed(&data, &mut out, &mut errs);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].counter, 7);
    }
}
