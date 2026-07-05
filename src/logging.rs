//! Env-variable-driven logging initialisation.

use log::LevelFilter;

/// Initialise the global logger from the `RUST_LOG` environment variable.
///
/// Falls back to `info` level if `RUST_LOG` is not set.
///
/// # Example
/// ```bash
/// RUST_LOG=debug cargo run
/// RUST_LOG=eeg_neurable_steamdeck=trace cargo run
/// ```
pub fn init() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .format_timestamp_millis()
    .init();
}

/// Initialise the logger with an explicit fallback level.
pub fn init_with_level(level: LevelFilter) {
    env_logger::Builder::new()
        .filter_level(level)
        .format_timestamp_millis()
        .init();
}
