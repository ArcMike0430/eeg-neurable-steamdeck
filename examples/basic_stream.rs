//! Example: Connect to a real or simulated MW75 and print EEG data to stdout.
//!
//! Run with:
//! ```bash
//! cargo run --example basic_stream
//! cargo run --example basic_stream -- --address AA:BB:CC:DD:EE:FF
//! ```

use anyhow::Result;
use eeg_neurable_steamdeck::{simulate::spawn_simulator, types::Mw75Event};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let addr = std::env::args().nth(2); // optional --address <MAC>

    let mut rx = if addr.is_some() {
        println!("Hardware mode not available in this example – using simulation.");
        spawn_simulator(500)
    } else {
        println!("Running in simulation mode (500 Hz, 12 channels)");
        spawn_simulator(500)
    };

    let mut count = 0u64;
    while let Some(event) = rx.recv().await {
        match event {
            Mw75Event::Connected { device_name, address } => {
                println!("Connected: {device_name} @ {address}");
            }
            Mw75Event::StreamStarted => {
                println!("Stream started. Printing first 10 packets…");
            }
            Mw75Event::Eeg(pkt) => {
                count += 1;
                if count <= 10 {
                    println!(
                        "[{count:>3}] counter={:>3}  ch[0]={:.3}µV  ch[11]={:.3}µV  status={:#04x}",
                        pkt.counter, pkt.channels[0], pkt.channels[11], pkt.status
                    );
                } else {
                    // Print every 500th packet (once per second)
                    if count % 500 == 0 {
                        println!("[{count}] Still streaming… ch[0]={:.3}µV", pkt.channels[0]);
                    }
                    if count >= 1500 {
                        println!("Done. Received {count} packets.");
                        break;
                    }
                }
            }
            Mw75Event::Battery(b) => {
                println!("Battery: {}% (charging: {})", b.level_pct, b.is_charging);
            }
            _ => {}
        }
    }

    Ok(())
}
