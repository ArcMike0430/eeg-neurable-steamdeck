//! EEG frame parser — validates and decodes little-endian MW75 frames.
//!
//! The MW75 sends 63-byte frames over RFCOMM channel 25.  All multi-byte
//! fields use **little-endian** byte order (REF, DRL, EEG channels, checksum).
//!
//! Gap-fill: if at most [`MAX_INTERP_GAP`] consecutive frames are missing the
//! parser linearly interpolates the voltages and marks them as estimated.
//! Larger gaps produce `f32::NAN` values so downstream consumers can detect
//! the discontinuity.

use crate::protocol::{
    EEG_CHANNEL_COUNT, FRAME_SIZE, MAX_INTERP_GAP, OFF_CHECKSUM, OFF_COUNTER, OFF_DRL, OFF_EEG,
    OFF_REF, OFF_STATUS, SOF_BYTE,
};
use byteorder::{ByteOrder, LE};
use thiserror::Error;

// ---- Types ------------------------------------------------------------------

/// A decoded EEG sample from a single MW75 frame.
#[derive(Debug, Clone, PartialEq)]
pub struct EegFrame {
    /// Rolling frame counter (wraps at 256).
    pub counter: u8,
    /// Reference electrode voltage (μV).
    pub ref_voltage: f32,
    /// Driven-right-leg electrode voltage (μV).
    pub drl_voltage: f32,
    /// 12-channel EEG voltages (μV).
    pub channels: [f32; EEG_CHANNEL_COUNT],
    /// Device status flags.
    pub status: u8,
    /// `true` if this frame was synthesised by gap-fill interpolation.
    pub interpolated: bool,
}

/// Running checksum / parse statistics.
#[derive(Debug, Default, Clone)]
pub struct ParseStats {
    pub frames_ok: u64,
    pub frames_bad_sof: u64,
    pub frames_bad_checksum: u64,
    pub gaps_filled: u64,
    pub gaps_lost: u64,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("buffer too short: got {got}, expected {expected}")]
    TooShort { got: usize, expected: usize },
    #[error("invalid SOF marker: bytes [0]={0:#04x} [1]={1:#04x}")]
    BadSof(u8, u8),
    #[error("checksum mismatch: computed {computed:#06x}, stored {stored:#06x}")]
    BadChecksum { computed: u16, stored: u16 },
}

// ---- Parser -----------------------------------------------------------------

/// Stateful frame parser — tracks the last counter for gap detection.
pub struct FrameParser {
    last_counter: Option<u8>,
    last_frame: Option<EegFrame>,
    pub stats: ParseStats,
}

impl FrameParser {
    pub fn new() -> Self {
        Self {
            last_counter: None,
            last_frame: None,
            stats: ParseStats::default(),
        }
    }

    /// Parse one raw 63-byte buffer.
    ///
    /// Returns `(Vec<EegFrame>, ...)`:
    /// - index 0 is the actual decoded frame
    /// - indices 1..n are gap-fill frames if frames were dropped
    pub fn parse(&mut self, buf: &[u8]) -> Result<Vec<EegFrame>, ParseError> {
        if buf.len() < FRAME_SIZE {
            return Err(ParseError::TooShort {
                got: buf.len(),
                expected: FRAME_SIZE,
            });
        }

        // Validate SOF
        if buf[0] != SOF_BYTE || buf[1] != SOF_BYTE {
            self.stats.frames_bad_sof += 1;
            return Err(ParseError::BadSof(buf[0], buf[1]));
        }

        // Validate checksum (sum of bytes 0..61, compared with u16-LE at 61)
        let computed = checksum(&buf[..OFF_CHECKSUM]);
        let stored = LE::read_u16(&buf[OFF_CHECKSUM..OFF_CHECKSUM + 2]);
        if computed != stored {
            self.stats.frames_bad_checksum += 1;
            return Err(ParseError::BadChecksum { computed, stored });
        }

        let frame = decode_frame(buf, false);
        self.stats.frames_ok += 1;

        // Gap detection
        let mut output = Vec::new();
        if let Some(last) = self.last_counter {
            let gap = frame.counter.wrapping_sub(last).wrapping_sub(1);
            if gap > 0 {
                let filled = fill_gap(self.last_frame.as_ref(), &frame, gap);
                if gap <= MAX_INTERP_GAP {
                    self.stats.gaps_filled += gap as u64;
                    output.extend(filled);
                } else {
                    self.stats.gaps_lost += gap as u64;
                    // Emit NaN frames to signal the discontinuity
                    for _ in 0..gap {
                        output.push(nan_frame(last.wrapping_add(1)));
                    }
                }
            }
        }

        self.last_counter = Some(frame.counter);
        self.last_frame = Some(frame.clone());
        output.push(frame);
        Ok(output)
    }
}

impl Default for FrameParser {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Internal helpers -------------------------------------------------------

fn decode_frame(buf: &[u8], interpolated: bool) -> EegFrame {
    let counter = buf[OFF_COUNTER];
    let ref_voltage = LE::read_f32(&buf[OFF_REF..]);
    let drl_voltage = LE::read_f32(&buf[OFF_DRL..]);
    let mut channels = [0f32; EEG_CHANNEL_COUNT];
    for (i, ch) in channels.iter_mut().enumerate() {
        *ch = LE::read_f32(&buf[OFF_EEG + i * 4..]);
    }
    let status = buf[OFF_STATUS];
    EegFrame {
        counter,
        ref_voltage,
        drl_voltage,
        channels,
        status,
        interpolated,
    }
}

fn fill_gap(prev: Option<&EegFrame>, next: &EegFrame, gap: u8) -> Vec<EegFrame> {
    let mut frames = Vec::with_capacity(gap as usize);
    let n = (gap + 1) as f32;
    for i in 1..=gap {
        let t = i as f32 / n;
        let mut channels = [0f32; EEG_CHANNEL_COUNT];
        if let Some(p) = prev {
            let ref_v = lerp(p.ref_voltage, next.ref_voltage, t);
            let drl_v = lerp(p.drl_voltage, next.drl_voltage, t);
            for (j, ch) in channels.iter_mut().enumerate() {
                *ch = lerp(p.channels[j], next.channels[j], t);
            }
            frames.push(EegFrame {
                counter: p.counter.wrapping_add(i),
                ref_voltage: ref_v,
                drl_voltage: drl_v,
                channels,
                status: p.status,
                interpolated: true,
            });
        } else {
            frames.push(nan_frame(next.counter.wrapping_sub(gap - i + 1)));
        }
    }
    frames
}

fn nan_frame(counter: u8) -> EegFrame {
    EegFrame {
        counter,
        ref_voltage: f32::NAN,
        drl_voltage: f32::NAN,
        channels: [f32::NAN; EEG_CHANNEL_COUNT],
        status: 0,
        interpolated: true,
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Compute checksum: simple sum of all bytes mod 2^16.
pub fn checksum(data: &[u8]) -> u16 {
    data.iter().fold(0u16, |acc, &b| acc.wrapping_add(b as u16))
}

// ---- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_frame(counter: u8, channels: [f32; EEG_CHANNEL_COUNT]) -> Vec<u8> {
        let mut buf = vec![0u8; FRAME_SIZE];
        buf[0] = SOF_BYTE;
        buf[1] = SOF_BYTE;
        buf[2] = 0x00;
        buf[OFF_COUNTER] = counter;
        LE::write_f32(&mut buf[OFF_REF..], 1.23);
        LE::write_f32(&mut buf[OFF_DRL..], 4.56);
        for (i, &ch) in channels.iter().enumerate() {
            LE::write_f32(&mut buf[OFF_EEG + i * 4..], ch);
        }
        buf[OFF_STATUS] = 0x01;
        let cs = checksum(&buf[..OFF_CHECKSUM]);
        LE::write_u16(&mut buf[OFF_CHECKSUM..], cs);
        buf
    }

    #[test]
    fn test_basic_parse() {
        let mut parser = FrameParser::new();
        let channels = [0.1f32; EEG_CHANNEL_COUNT];
        let buf = make_frame(0, channels);
        let frames = parser.parse(&buf).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].counter, 0);
        assert!((frames[0].ref_voltage - 1.23).abs() < 1e-5);
        assert!(!frames[0].interpolated);
    }

    #[test]
    fn test_gap_fill_small() {
        let mut parser = FrameParser::new();
        let channels = [0.0f32; EEG_CHANNEL_COUNT];
        // Frame 0
        let buf0 = make_frame(0, channels);
        parser.parse(&buf0).unwrap();
        // Frame 3 (gap of 2: counters 1 and 2 are missing)
        let buf3 = make_frame(3, channels);
        let frames = parser.parse(&buf3).unwrap();
        // Should have 2 interpolated + 1 real = 3 total
        assert_eq!(frames.len(), 3);
        assert!(frames[0].interpolated);
        assert!(frames[1].interpolated);
        assert!(!frames[2].interpolated);
        assert_eq!(parser.stats.gaps_filled, 2);
    }

    #[test]
    fn test_gap_fill_large() {
        let mut parser = FrameParser::new();
        let channels = [0.0f32; EEG_CHANNEL_COUNT];
        let buf0 = make_frame(0, channels);
        parser.parse(&buf0).unwrap();
        // Frame 10 — gap of 9, exceeds MAX_INTERP_GAP=3
        let buf10 = make_frame(10, channels);
        let frames = parser.parse(&buf10).unwrap();
        assert_eq!(frames.len(), 10); // 9 NaN + 1 real
        for f in &frames[..9] {
            assert!(f.ref_voltage.is_nan());
        }
        assert_eq!(parser.stats.gaps_lost, 9);
    }

    #[test]
    fn test_bad_checksum() {
        let mut parser = FrameParser::new();
        let channels = [0.0f32; EEG_CHANNEL_COUNT];
        let mut buf = make_frame(0, channels);
        buf[OFF_CHECKSUM] ^= 0xFF; // corrupt checksum
        assert!(parser.parse(&buf).is_err());
        assert_eq!(parser.stats.frames_bad_checksum, 1);
    }

    #[test]
    fn test_bad_sof() {
        let mut parser = FrameParser::new();
        let channels = [0.0f32; EEG_CHANNEL_COUNT];
        let mut buf = make_frame(0, channels);
        buf[0] = 0x00;
        assert!(parser.parse(&buf).is_err());
        assert_eq!(parser.stats.frames_bad_sof, 1);
    }
}
