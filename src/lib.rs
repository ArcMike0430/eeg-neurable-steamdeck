//! Public library API for the EEG acquisition pipeline.
//!
//! # Quick start
//!
//! ```no_run
//! use eeg_neurable_steamdeck::{
//!     simulate::spawn_simulator,
//!     output::{run_outputs, OutputConfig},
//! };
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let rx = spawn_simulator(500);
//!     let cfg = OutputConfig {
//!         csv_path: Some(PathBuf::from("eeg_data.csv")),
//!         ..Default::default()
//!     };
//!     run_outputs(cfg, rx).await
//! }
//! ```

pub mod logging;
pub mod mw75_client;
pub mod output;
pub mod parse;
pub mod protocol;
pub mod rfcomm;
pub mod simulate;
pub mod types;

// Re-export the most commonly used types for ergonomic access.
pub use types::{BatteryInfo, EegPacket, EegSample, Mw75Event, ProtocolError, StreamConfig};
