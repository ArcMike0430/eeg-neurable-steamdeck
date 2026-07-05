use std::thread;
use std::time::{Duration, Instant};

use rand::Rng;

use crate::protocol::{checksum_u16_le, PACKET_SIZE, SYNC_BYTE};

#[derive(Debug, Clone)]
pub struct SimulationConfig {
    pub drop_rate: f32,
    pub jitter_ms: u64,
    pub corrupt_rate: f32,
    pub timeout_after: Option<Duration>,
    pub stress_reconnect: bool,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            drop_rate: 0.0,
            jitter_ms: 0,
            corrupt_rate: 0.01,
            timeout_after: None,
            stress_reconnect: false,
        }
    }
}

pub struct MockSimulator {
    cfg: SimulationConfig,
    counter: u8,
    start: Instant,
}

impl MockSimulator {
    pub fn new(cfg: SimulationConfig) -> Self {
        Self {
            cfg,
            counter: 0,
            start: Instant::now(),
        }
    }

    pub fn next_frame(&mut self) -> Option<Vec<u8>> {
        if let Some(timeout) = self.cfg.timeout_after {
            if self.start.elapsed() > timeout {
                if self.cfg.stress_reconnect {
                    self.start = Instant::now();
                }
                return None;
            }
        }

        let mut rng = rand::thread_rng();
        if rng.gen_bool(self.cfg.drop_rate.clamp(0.0, 1.0) as f64) {
            return None;
        }
        if self.cfg.jitter_ms > 0 {
            let jitter = rng.gen_range(0..=self.cfg.jitter_ms);
            thread::sleep(Duration::from_millis(jitter));
        }

        let mut packet = build_clean_packet(self.counter);
        self.counter = self.counter.wrapping_add(1);

        if rng.gen_bool(self.cfg.corrupt_rate.clamp(0.0, 1.0) as f64) {
            let idx = rng.gen_range(0..packet.len() - 2);
            packet[idx] ^= 0xA5;
        }

        Some(packet)
    }
}

fn build_clean_packet(counter: u8) -> Vec<u8> {
    let mut packet = vec![0u8; PACKET_SIZE];
    packet[0] = SYNC_BYTE;
    packet[1] = 0x10;
    packet[2] = 0x39;
    packet[3] = counter;

    let t = counter as f32 / 500.0;
    packet[4..8]
        .copy_from_slice(&(50.0 * (2.0 * std::f32::consts::PI * 10.0 * t).sin()).to_le_bytes());
    packet[8..12]
        .copy_from_slice(&(50.0 * (2.0 * std::f32::consts::PI * 6.0 * t).sin()).to_le_bytes());

    let mut offset = 12;
    for ch in 0..12 {
        let alpha = 30.0 * (2.0 * std::f32::consts::PI * 10.0 * t + ch as f32 * 0.1).sin();
        let beta = 15.0 * (2.0 * std::f32::consts::PI * 20.0 * t + ch as f32 * 0.2).sin();
        let theta = 8.0 * (2.0 * std::f32::consts::PI * 6.0 * t + ch as f32 * 0.15).sin();
        let value = alpha + beta + theta;
        packet[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        offset += 4;
    }

    packet[60] = 0x01;
    let checksum = checksum_u16_le(&packet[..PACKET_SIZE - 2]);
    packet[61..63].copy_from_slice(&checksum.to_le_bytes());
    packet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulator_can_drop_everything() {
        let mut sim = MockSimulator::new(SimulationConfig {
            drop_rate: 1.0,
            ..SimulationConfig::default()
        });
        assert_eq!(sim.next_frame(), None);
    }

    #[test]
    fn simulator_emits_packet_without_corruption() {
        let mut sim = MockSimulator::new(SimulationConfig {
            corrupt_rate: 0.0,
            ..SimulationConfig::default()
        });
        let packet = sim.next_frame().expect("frame");
        assert_eq!(packet.len(), PACKET_SIZE);
        assert_eq!(packet[0], SYNC_BYTE);
    }
}
