//! `eeg-stream` – headless EEG data collector.
//!
//! Streams raw EEG data from the Neurable MW75 to CSV, WebSocket, and/or LSL.
//!
//! ```bash
//! # Simulation mode
//! eeg-stream --simulate -o output.csv
//!
//! # Hardware mode (requires rfcomm feature)
//! eeg-stream --address AA:BB:CC:DD:EE:FF -o output.csv
//! ```

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
    name = "eeg-stream",
    version,
    about = "Headless EEG data streamer for Neurable MW75"
)]
struct Args {
    /// Bluetooth MAC address of the MW75
    #[arg(short, long, env = "MW75_ADDRESS")]
    address: Option<String>,

    /// Output CSV file path
    #[arg(short = 'o', long, default_value = "eeg_data.csv")]
    csv: PathBuf,

    /// Enable simulation mode
    #[arg(long)]
    simulate: bool,

    /// RFCOMM channel (default: 25)
    #[arg(long, default_value_t = 25)]
    channel: u8,

    /// WebSocket bind address (requires websocket feature)
    #[cfg(feature = "websocket")]
    #[arg(long)]
    ws_addr: Option<std::net::SocketAddr>,

    /// Enable LSL output (requires lsl feature)
    #[cfg(feature = "lsl")]
    #[arg(long)]
    lsl: bool,

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
        log::info!("Simulation mode");
        spawn_simulator(config.sample_rate_hz)
    } else {
        #[cfg(feature = "rfcomm")]
        {
            use eeg_neurable_steamdeck::{mw75_client, rfcomm};
            use tokio::sync::mpsc;

            let (tx, rx) = mpsc::channel(1024);
            let addr = args.address.clone().unwrap();
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
            log::warn!("rfcomm feature not enabled – using simulation");
            spawn_simulator(config.sample_rate_hz)
        }
    };

    let out_cfg = OutputConfig {
        csv_path: Some(args.csv),
        #[cfg(feature = "websocket")]
        websocket_addr: args.ws_addr,
        #[cfg(feature = "lsl")]
        lsl_enabled: args.lsl,
    };

    run_outputs(out_cfg, rx).await
}
