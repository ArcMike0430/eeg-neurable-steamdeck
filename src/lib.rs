//! eeg-neurable-steamdeck — library crate root.
//!
//! Provides the full MW75 EEG acquisition stack:
//! - Protocol constants (correct vendor UUIDs, 5-byte GAIA commands)
//! - Little-endian frame parser with gap-fill
//! - BLE client (btleplug, USB-BT500 preferred)
//! - RFCOMM reader with ring-buffer backpressure + stall detection
//! - GAIA keep-alive watchdog (RFCOMM ch1 heartbeat monitoring)
//! - Peer sync via UDP multicast (224.0.0.1:5005)
//! - Simulation / mock mode with failure injection
//! - Configuration profiles (Steam Deck, Jetson Orin)

pub mod adapter;
pub mod config;
pub mod keepalive;
pub mod mw75_client;
pub mod parse;
pub mod protocol;
pub mod rfcomm;
pub mod simulate;
pub mod sync_peer;

/// Re-exports most commonly used types.
pub mod prelude {
    pub use crate::config::Config;
    pub use crate::keepalive::{ConnectionState, HeartbeatEvent, KeepAliveWatchdog};
    pub use crate::parse::{EegFrame, FrameParser, ParseStats};
    pub use crate::protocol::*;
    pub use crate::rfcomm::{FrameRingBuffer, RfcommReader};
    pub use crate::simulate::{EegSimulator, SimConfig};
    pub use crate::sync_peer::PeerSync;
}

/// Initialise the logger from the config level string.
pub fn init_logger(level: &str) {
    let filter = level.parse().unwrap_or(log::LevelFilter::Info);
    env_logger::builder()
        .filter_level(filter)
        .format_timestamp_millis()
        .init();
}
