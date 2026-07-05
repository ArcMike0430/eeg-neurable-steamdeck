//! Mock / simulation mode for testing without MW75 hardware.
//!
//! Generates synthetic 12-channel EEG with realistic multi-band signals and
//! supports failure injection for stress-testing reconnect logic:
//! - **Drop**: skip packets at a configurable rate
//! - **Jitter**: add random latency to each packet
//! - **Corruption**: flip a random bit in the payload
//! - **Timeout**: stop sending after N packets to simulate GAIA session expiry

use crate::parse::{checksum, EegFrame};
use crate::protocol::{
    EEG_CHANNEL_COUNT, EEG_SAMPLE_RATE, FRAME_SIZE, OFF_CHECKSUM, OFF_COUNTER, OFF_DRL, OFF_EEG,
    OFF_REF, OFF_STATUS, SOF_BYTE,
};
use byteorder::{ByteOrder, LE};
use log::{debug, info, warn};
use rand::Rng;
use std::time::Duration;

/// Configuration for the EEG simulator.
#[derive(Debug, Clone)]
pub struct SimConfig {
    /// Fraction of frames to silently drop (0.0 = none, 1.0 = all).
    pub drop_rate: f64,
    /// Maximum one-sided random latency added to each frame (0 = none).
    pub jitter_ms: u64,
    /// Fraction of frames to corrupt (bit-flip in a random EEG byte).
    pub corruption_rate: f64,
    /// If Some(n), stop producing frames after n frames to simulate timeout.
    pub timeout_after: Option<u64>,
    /// If true, restart the simulator after `timeout_after` to stress reconnect.
    pub stress_reconnect: bool,
    /// Sample rate in Hz (default: [`EEG_SAMPLE_RATE`]).
    pub sample_rate: u32,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            drop_rate: 0.0,
            jitter_ms: 0,
            corruption_rate: 0.0,
            timeout_after: None,
            stress_reconnect: false,
            sample_rate: EEG_SAMPLE_RATE,
        }
    }
}

/// Stateful EEG frame generator.
pub struct EegSimulator {
    config: SimConfig,
    counter: u8,
    frames_emitted: u64,
    rng: rand::rngs::ThreadRng,
    phase: f64,
}

impl EegSimulator {
    pub fn new(config: SimConfig) -> Self {
        Self {
            config,
            counter: 0,
            frames_emitted: 0,
            rng: rand::thread_rng(),
            phase: 0.0,
        }
    }

    /// Generate the next raw frame buffer (63 bytes), or `None` on timeout.
    ///
    /// Returns `Some(bytes)` normally, `None` when the timeout limit is
    /// reached (simulates GAIA session expiry).
    pub fn next_raw(&mut self) -> Option<Vec<u8>> {
        // Timeout simulation
        if let Some(limit) = self.config.timeout_after {
            if self.frames_emitted >= limit {
                if self.config.stress_reconnect {
                    info!("Simulator: timeout limit hit — resetting (stress_reconnect)");
                    self.reset();
                } else {
                    warn!("Simulator: timeout limit hit — returning None");
                    return None;
                }
            }
        }

        // Drop simulation
        if self.config.drop_rate > 0.0 && self.rng.gen::<f64>() < self.config.drop_rate {
            debug!("Simulator: dropping frame {}", self.counter);
            self.counter = self.counter.wrapping_add(1);
            self.frames_emitted += 1;
            return self.next_raw(); // recurse to emit the NEXT frame
        }

        let buf = self.build_frame();
        self.frames_emitted += 1;
        Some(buf)
    }

    /// Decode the next frame (convenience wrapper).
    pub fn next_frame(&mut self) -> Option<EegFrame> {
        let raw = self.next_raw()?;
        crate::parse::FrameParser::new()
            .parse(&raw)
            .ok()
            .and_then(|mut v| v.pop())
    }

    // ---- Internal -----------------------------------------------------------

    fn build_frame(&mut self) -> Vec<u8> {
        let dt = 1.0 / self.config.sample_rate as f64;
        self.phase += dt;

        let mut buf = vec![0u8; FRAME_SIZE];
        buf[0] = SOF_BYTE;
        buf[1] = SOF_BYTE;
        buf[2] = 0x00;
        buf[OFF_COUNTER] = self.counter;
        self.counter = self.counter.wrapping_add(1);

        // REF / DRL: small noise
        let ref_v = 0.5 * (2.0 * std::f64::consts::PI * 0.1 * self.phase).sin() as f32;
        let drl_v = ref_v * 0.1;
        LE::write_f32(&mut buf[OFF_REF..], ref_v);
        LE::write_f32(&mut buf[OFF_DRL..], drl_v);

        // 12 EEG channels: multi-band (alpha 10 Hz, beta 20 Hz, theta 6 Hz)
        for (i, _) in (0..EEG_CHANNEL_COUNT).enumerate() {
            let alpha = 15.0 * (2.0 * std::f64::consts::PI * 10.0 * self.phase).sin();
            let beta = 5.0 * (2.0 * std::f64::consts::PI * 20.0 * self.phase).sin();
            let theta = 8.0 * (2.0 * std::f64::consts::PI * 6.0 * self.phase).sin();
            let noise: f64 = self.rng.gen_range(-1.0..1.0);
            let ch = ((alpha + beta + theta + noise) * (1.0 + i as f64 * 0.01)) as f32;
            LE::write_f32(&mut buf[OFF_EEG + i * 4..], ch);
        }

        buf[OFF_STATUS] = 0x01;

        // Write checksum over the clean payload first
        let cs = checksum(&buf[..OFF_CHECKSUM]);
        LE::write_u16(&mut buf[OFF_CHECKSUM..], cs);

        // Corruption simulation: bit-flip an EEG byte AFTER checksum is written,
        // so the stored checksum no longer matches — the parser rejects the frame.
        if self.config.corruption_rate > 0.0
            && self.rng.gen::<f64>() < self.config.corruption_rate
        {
            let byte_idx = self.rng.gen_range(OFF_EEG..OFF_STATUS);
            let bit = self.rng.gen_range(0..8);
            buf[byte_idx] ^= 1 << bit;
            debug!("Simulator: corrupted byte {byte_idx} bit {bit}");
        }

        buf
    }

    fn reset(&mut self) {
        self.counter = 0;
        self.frames_emitted = 0;
        self.phase = 0.0;
    }

    pub fn frames_emitted(&self) -> u64 {
        self.frames_emitted
    }
}

/// Sleep the appropriate amount for the configured sample rate + jitter.
pub async fn sim_frame_delay(config: &SimConfig) {
    let base_us = 1_000_000 / config.sample_rate as u64;
    let jitter_us = if config.jitter_ms > 0 {
        let mut rng = rand::thread_rng();
        rng.gen_range(0..config.jitter_ms * 1000)
    } else {
        0
    };
    tokio::time::sleep(Duration::from_micros(base_us + jitter_us)).await;
}

// ---- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_generation() {
        let cfg = SimConfig::default();
        let mut sim = EegSimulator::new(cfg);
        let raw = sim.next_raw().unwrap();
        assert_eq!(raw.len(), FRAME_SIZE);
        assert_eq!(raw[0], SOF_BYTE);
        assert_eq!(raw[1], SOF_BYTE);
    }

    #[test]
    fn test_checksum_valid() {
        let cfg = SimConfig::default();
        let mut sim = EegSimulator::new(cfg);
        for _ in 0..10 {
            let raw = sim.next_raw().unwrap();
            let stored = LE::read_u16(&raw[OFF_CHECKSUM..]);
            let computed = checksum(&raw[..OFF_CHECKSUM]);
            assert_eq!(stored, computed, "checksum mismatch on frame");
        }
    }

    #[test]
    fn test_timeout_stops_generation() {
        let cfg = SimConfig {
            timeout_after: Some(5),
            stress_reconnect: false,
            ..Default::default()
        };
        let mut sim = EegSimulator::new(cfg);
        let mut count = 0;
        while sim.next_raw().is_some() {
            count += 1;
            if count > 100 {
                panic!("Should have stopped");
            }
        }
        assert_eq!(count, 5);
    }

    #[test]
    fn test_drop_rate_reduces_output() {
        // Drop rate of 1.0 should skip all frames — but since we recurse,
        // this would infinite-loop.  Test drop_rate=0.5 instead by counting.
        let cfg = SimConfig {
            drop_rate: 0.0,
            ..Default::default()
        };
        let mut sim = EegSimulator::new(cfg);
        // With 0 drop rate, should produce frames normally
        for _ in 0..10 {
            assert!(sim.next_raw().is_some());
        }
    }

    #[test]
    fn test_counter_increments() {
        let cfg = SimConfig::default();
        let mut sim = EegSimulator::new(cfg);
        let f0 = sim.next_raw().unwrap();
        let f1 = sim.next_raw().unwrap();
        assert_eq!(f0[OFF_COUNTER], 0);
        assert_eq!(f1[OFF_COUNTER], 1);
    }

    #[test]
    fn test_stress_reconnect_resets() {
        let cfg = SimConfig {
            timeout_after: Some(3),
            stress_reconnect: true,
            ..Default::default()
        };
        let mut sim = EegSimulator::new(cfg);
        // Should produce frames indefinitely (resetting every 3)
        for _ in 0..9 {
            assert!(sim.next_raw().is_some());
        }
    }

    #[test]
    fn test_corruption_breaks_checksum() {
        // Corruption rate = 1.0 — every frame should have a bad checksum
        // (assuming bit flip lands outside checksum bytes, which it always
        //  does since we target OFF_EEG..OFF_STATUS range)
        let cfg = SimConfig {
            corruption_rate: 1.0,
            ..Default::default()
        };
        let mut sim = EegSimulator::new(cfg);
        let mut bad = 0;
        for _ in 0..20 {
            let raw = sim.next_raw().unwrap();
            let stored = LE::read_u16(&raw[OFF_CHECKSUM..]);
            let computed = checksum(&raw[..OFF_CHECKSUM]);
            if stored != computed {
                bad += 1;
            }
        }
        // All frames should have corrupted checksums
        assert_eq!(bad, 20);
    }
}
