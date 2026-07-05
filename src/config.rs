//! Configuration management — load Steam Deck / Jetson Orin profiles from
//! TOML files, with sensible defaults.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub device: DeviceConfig,
    pub ble: BleConfig,
    pub rfcomm: RfcommConfig,
    pub sync: SyncConfig,
    pub simulation: SimulationConfig,
    pub logging: LoggingConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device: DeviceConfig::default(),
            ble: BleConfig::default(),
            rfcomm: RfcommConfig::default(),
            sync: SyncConfig::default(),
            simulation: SimulationConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

/// Device / platform identification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceConfig {
    /// Human-readable name for this node (e.g. "steam-deck-1").
    pub name: String,
    /// Platform tag: "steam-deck" | "jetson-orin" | "generic"
    pub platform: String,
    /// Force a specific BLE adapter by hciN name (e.g. "hci1").
    /// If empty, adapter.rs auto-selects USB-BT500 / best available.
    pub preferred_adapter: String,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            name: "eeg-node".to_string(),
            platform: "generic".to_string(),
            preferred_adapter: String::new(),
        }
    }
}

/// BLE / GAIA connection settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BleConfig {
    /// BLE scan timeout in seconds.
    pub scan_timeout_secs: u64,
    /// BLE adapter index to pass to btleplug (overridden by preferred_adapter
    /// if that is set).
    pub adapter_index: Option<usize>,
}

impl Default for BleConfig {
    fn default() -> Self {
        Self {
            scan_timeout_secs: 10,
            adapter_index: None,
        }
    }
}

/// RFCOMM / EEG streaming settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RfcommConfig {
    /// RFCOMM channel for EEG data.
    pub eeg_channel: u8,
    /// RFCOMM channel for heartbeat / keep-alive.
    pub heartbeat_channel: u8,
    /// Ring-buffer capacity in frames.
    pub ring_buffer_size: usize,
    /// Stall detection threshold in ms.
    pub stall_threshold_ms: u64,
}

impl Default for RfcommConfig {
    fn default() -> Self {
        Self {
            eeg_channel: crate::protocol::RFCOMM_EEG_CHANNEL,
            heartbeat_channel: crate::protocol::RFCOMM_HEARTBEAT_CHANNEL,
            ring_buffer_size: crate::protocol::RING_BUFFER_FRAMES,
            stall_threshold_ms: crate::protocol::STALL_THRESHOLD_MS,
        }
    }
}

/// Peer-sync (UDP multicast) settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncConfig {
    /// Enable peer sync broadcasting.
    pub enabled: bool,
    /// Multicast group address.
    pub multicast_addr: String,
    /// UDP port.
    pub port: u16,
    /// Epoch length in seconds.
    pub epoch_secs: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            multicast_addr: crate::protocol::SYNC_MULTICAST_ADDR.to_string(),
            port: crate::protocol::SYNC_PORT,
            epoch_secs: crate::protocol::SYNC_EPOCH_SECS,
        }
    }
}

/// Mock / simulation settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SimulationConfig {
    /// Enable mock mode (no real hardware needed).
    pub enabled: bool,
    /// Packet drop rate (0.0–1.0).
    pub drop_rate: f64,
    /// Maximum random latency per frame in ms.
    pub jitter_ms: u64,
    /// Packet corruption rate (0.0–1.0).
    pub corruption_rate: f64,
    /// Stop after this many frames (simulate GAIA timeout).
    pub timeout_after: Option<u64>,
    /// Restart after timeout (stress-test reconnect).
    pub stress_reconnect: bool,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            drop_rate: 0.0,
            jitter_ms: 0,
            corruption_rate: 0.0,
            timeout_after: None,
            stress_reconnect: false,
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Log level: "error" | "warn" | "info" | "debug" | "trace"
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
        }
    }
}

// ---- I/O --------------------------------------------------------------------

/// Load a config from a TOML file, merging missing fields with defaults.
pub fn load(path: impl AsRef<Path>) -> Result<Config> {
    let content = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("read config {}", path.as_ref().display()))?;
    let cfg: Config = toml::from_str(&content)
        .with_context(|| format!("parse config {}", path.as_ref().display()))?;
    Ok(cfg)
}

/// Save a config to a TOML file.
pub fn save(cfg: &Config, path: impl AsRef<Path>) -> Result<()> {
    let content = toml::to_string_pretty(cfg).context("serialise config")?;
    std::fs::write(path.as_ref(), content)
        .with_context(|| format!("write config {}", path.as_ref().display()))?;
    Ok(())
}

// ---- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_round_trips() {
        let cfg = Config::default();
        let serialised = toml::to_string_pretty(&cfg).unwrap();
        let loaded: Config = toml::from_str(&serialised).unwrap();
        assert_eq!(loaded.device.platform, cfg.device.platform);
        assert_eq!(loaded.rfcomm.eeg_channel, cfg.rfcomm.eeg_channel);
    }

    #[test]
    fn test_load_from_string() {
        let toml_str = r#"
[device]
name = "test-deck"
platform = "steam-deck"

[simulation]
enabled = true
drop_rate = 0.05
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.device.name, "test-deck");
        assert_eq!(cfg.device.platform, "steam-deck");
        assert!((cfg.simulation.drop_rate - 0.05).abs() < 1e-9);
        assert!(cfg.simulation.enabled);
    }
}
