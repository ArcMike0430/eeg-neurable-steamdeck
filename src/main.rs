//! Headless EEG streaming daemon – CSV / WebSocket / LSL output.
//!
//! Run with `cargo run` (or the compiled binary) for the default TUI, or use
//! the `eeg-stream` binary for headless output.

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

use eeg_neurable_steamdeck::{
    logging,
    output::{run_outputs, OutputConfig},
    simulate::spawn_simulator,
    types::StreamConfig,
};

#[derive(Parser, Debug)]
#[command(
    name = "eeg-neurable",
    version,
    about = "Neurable MW75 raw EEG streaming daemon for Steam Deck / Jetson Orin"
)]
struct Args {
    /// Bluetooth MAC address of the MW75 (omit to use simulation mode)
    #[arg(short, long, env = "MW75_ADDRESS")]
    address: Option<String>,

    /// Output CSV file path
    #[arg(short = 'o', long, default_value = "eeg_data.csv")]
    csv: PathBuf,

    /// Enable simulation mode (no hardware required)
    #[arg(long)]
    simulate: bool,

    /// RFCOMM channel (default: 25)
    #[arg(long, default_value_t = 25)]
    channel: u8,

    /// WebSocket bind address (e.g. 0.0.0.0:8765) – requires websocket feature
    #[arg(long)]
    ws_addr: Option<String>,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if args.verbose {
        logging::init_with_level(log::LevelFilter::Debug);
    } else {
        logging::init();
    }

    let simulate = args.simulate || args.address.is_none();

    let config = StreamConfig {
        device_address: args.address.clone(),
        rfcomm_channel: args.channel,
        simulate,
        ..Default::default()
    };

    let rx = if simulate {
        log::info!("Running in simulation mode");
        spawn_simulator(config.sample_rate_hz)
    } else {
        // Hardware path: BLE activation + RFCOMM streaming
        #[cfg(feature = "rfcomm")]
        {
            use eeg_neurable_steamdeck::{mw75_client, rfcomm};
            use tokio::sync::mpsc;

            let (tx, rx) = mpsc::channel(1024);
            let addr = args.address.unwrap();
            let channel = args.channel;

            tokio::spawn(async move {
                let peripheral =
                    mw75_client::discover_mw75(std::time::Duration::from_secs(15)).await?;
                mw75_client::activate_eeg(&peripheral).await?;
                rfcomm::stream_rfcomm(&addr, channel, tx).await
            });

            rx
        }
        #[cfg(not(feature = "rfcomm"))]
        {
            log::warn!("rfcomm feature not enabled – falling back to simulation");
            spawn_simulator(config.sample_rate_hz)
        }
    };

    let out_cfg = OutputConfig {
        csv_path: Some(args.csv),
        ..Default::default()
    };

    run_outputs(out_cfg, rx).await
}
