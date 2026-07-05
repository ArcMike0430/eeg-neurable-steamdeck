//! Simulation mode – generates realistic synthetic EEG data without hardware.
//!
//! Enabled via the `simulation` Cargo feature or by setting
//! `StreamConfig::simulate = true`.

use std::f32::consts::PI;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::mpsc;
use tokio::time;

use crate::protocol::EEG_CHANNEL_COUNT;
use crate::types::{BatteryInfo, EegPacket, Mw75Event};

// ── Simulator ────────────────────────────────────────────────────────────────

/// Generates synthetic EEG frames at the nominal 500 Hz rate.
pub struct EegSimulator {
    sample_rate: u32,
    counter: u8,
    phase: [f32; EEG_CHANNEL_COUNT],
    freq: [f32; EEG_CHANNEL_COUNT],
    amplitude: [f32; EEG_CHANNEL_COUNT],
    battery_level: u8,
}

impl EegSimulator {
    /// Create a new simulator with realistic per-channel sine-wave parameters.
    pub fn new(sample_rate: u32) -> Self {
        // Frequency bands roughly matching alpha (8-12 Hz) and beta (13-30 Hz)
        let freq = [
            10.0, 12.0, 8.5, 11.0, // frontal alpha
            20.0, 18.0, 15.0, 25.0, // central beta
            9.5, 10.5, 22.0, 16.0, // parietal/occipital
        ];
        // Amplitude in µV (realistic EEG: ~5-50 µV)
        let amplitude = [
            20.0, 18.0, 25.0, 22.0, 15.0, 17.0, 30.0, 12.0, 14.0, 19.0, 11.0, 16.0,
        ];
        Self {
            sample_rate,
            counter: 0,
            phase: [0.0; EEG_CHANNEL_COUNT],
            freq,
            amplitude,
            battery_level: 80,
        }
    }

    /// Generate one EEG sample.
    fn next_sample(&mut self) -> EegPacket {
        let dt = 1.0 / self.sample_rate as f32;
        let mut channels = [0f32; EEG_CHANNEL_COUNT];

        for i in 0..EEG_CHANNEL_COUNT {
            // Base sine + small noise
            let noise = pseudo_noise(self.counter as usize + i) * 2.0;
            channels[i] = self.amplitude[i] * (2.0 * PI * self.freq[i] * self.phase[i]).sin()
                + noise;
            self.phase[i] += dt;
            if self.phase[i] > 1.0 {
                self.phase[i] -= 1.0;
            }
        }

        let pkt = EegPacket {
            timestamp: Utc::now(),
            counter: self.counter,
            ref_uv: 0.0,
            drl_uv: 0.0,
            channels,
            status: 0x00,
            checksum: 0x0000, // simulated – no real checksum
        };
        self.counter = self.counter.wrapping_add(1);
        pkt
    }

    /// Run the simulator, sending events to the provided channel until the
    /// channel is closed or a task cancellation occurs.
    pub async fn run(mut self, tx: mpsc::Sender<Mw75Event>) {
        let interval = Duration::from_nanos(1_000_000_000 / self.sample_rate as u64);
        let mut ticker = time::interval(interval);

        // Signal connected
        let _ = tx
            .send(Mw75Event::Connected {
                device_name: "MW75-SIM".to_string(),
                address: "00:00:00:00:00:00".to_string(),
            })
            .await;
        let _ = tx.send(Mw75Event::StreamStarted).await;

        let mut battery_tick = 0u32;

        loop {
            ticker.tick().await;
            let pkt = self.next_sample();
            if tx.send(Mw75Event::Eeg(pkt)).await.is_err() {
                break;
            }

            // Send a battery update every ~5 seconds
            battery_tick += 1;
            if battery_tick >= self.sample_rate * 5 {
                battery_tick = 0;
                self.battery_level = self.battery_level.saturating_sub(1).max(1);
                let _ = tx
                    .send(Mw75Event::Battery(BatteryInfo {
                        level_pct: self.battery_level,
                        is_charging: false,
                        timestamp: Utc::now(),
                    }))
                    .await;
            }
        }

        let _ = tx.send(Mw75Event::StreamStopped).await;
    }
}

/// Spawn a simulator task, returning a receiver for events.
pub fn spawn_simulator(sample_rate: u32) -> mpsc::Receiver<Mw75Event> {
    let (tx, rx) = mpsc::channel(1024);
    let sim = EegSimulator::new(sample_rate);
    tokio::spawn(sim.run(tx));
    rx
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// A deterministic pseudo-noise function in range [-1, 1].
fn pseudo_noise(seed: usize) -> f32 {
    let x = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    let x = x & 0x7fff_ffff;
    (x as f32 / 0x3fff_ffff as f32) - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulator_generates_samples() {
        let mut sim = EegSimulator::new(500);
        let s1 = sim.next_sample();
        let s2 = sim.next_sample();
        assert_eq!(s1.counter, 0);
        assert_eq!(s2.counter, 1);
        // Channels should not all be zero
        assert!(s1.channels.iter().any(|&v| v != 0.0));
    }

    #[tokio::test]
    async fn simulator_stream_delivers_events() {
        let mut rx = spawn_simulator(500);
        // Collect a few events with a short timeout
        let mut got = Vec::new();
        for _ in 0..5 {
            if let Ok(ev) = tokio::time::timeout(
                Duration::from_millis(50),
                rx.recv(),
            )
            .await
            {
                got.push(ev);
            }
        }
        // At minimum we should see Connected + StreamStarted + at least one EEG frame
        assert!(got.len() >= 3, "expected >= 3 events, got {}", got.len());
    }
}
