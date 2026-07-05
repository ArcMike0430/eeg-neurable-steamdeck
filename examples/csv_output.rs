//! Example: Write EEG data to a CSV file using the output module.
//!
//! ```bash
//! cargo run --example csv_output
//! ```

use anyhow::Result;
use std::path::PathBuf;

use eeg_neurable_steamdeck::{
    output::{run_outputs, OutputConfig},
    simulate::spawn_simulator,
};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .init();

    let path = PathBuf::from("/tmp/eeg_example.csv");
    println!("Writing EEG data to {path:?} for 2 seconds (1 000 packets)…");

    // Start the simulator
    let (tx, rx) = tokio::sync::mpsc::channel(1024);
    let sim_rx = spawn_simulator(500);

    // Forward first 1 000 EEG packets then close the channel
    tokio::spawn(async move {
        let mut fwd = sim_rx;
        let mut count = 0u64;
        while let Some(ev) = fwd.recv().await {
            let is_eeg = matches!(ev, eeg_neurable_steamdeck::types::Mw75Event::Eeg(_));
            if tx.send(ev).await.is_err() {
                break;
            }
            if is_eeg {
                count += 1;
                if count >= 1000 {
                    break;
                }
            }
        }
    });

    let cfg = OutputConfig {
        csv_path: Some(path.clone()),
        ..Default::default()
    };

    run_outputs(cfg, rx).await?;

    println!("Done. File written to {path:?}");
    Ok(())
}
