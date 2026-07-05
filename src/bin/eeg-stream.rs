use std::time::Duration;

use clap::Parser;
use eeg_neurable_steamdeck::logging::init_logging;
use eeg_neurable_steamdeck::output::CsvWriter;
use eeg_neurable_steamdeck::parse::PacketParser;
use eeg_neurable_steamdeck::rfcomm::RfcommReader;
use eeg_neurable_steamdeck::simulate::{MockSimulator, SimulationConfig};
use eeg_neurable_steamdeck::Mw75Client;
use log::{info, warn};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    mock: bool,
    #[arg(long, default_value_t = 0.0)]
    drop_rate: f32,
    #[arg(long, default_value_t = 0)]
    jitter: u64,
    #[arg(long, default_value_t = 0.01)]
    corrupt: f32,
    #[arg(long)]
    timeout_after: Option<u64>,
    #[arg(long)]
    stress_reconnect: bool,
    #[arg(long, default_value = "eeg.csv")]
    csv: String,
}

fn main() -> anyhow::Result<()> {
    init_logging();
    let args = Args::parse();

    let mut client = Mw75Client::default();
    let adapter = client.select_adapter()?;
    info!("selected adapter: {}", adapter.name);

    if !args.mock {
        if let Err(err) = client.activate_gaia() {
            warn!("BLE activation failed: {err}. If using Jetson internal adapter this is expected; use USB-BT500");
            return Ok(());
        }
    }

    let sim_cfg = SimulationConfig {
        drop_rate: args.drop_rate,
        jitter_ms: args.jitter,
        corrupt_rate: args.corrupt,
        timeout_after: args.timeout_after.map(Duration::from_secs),
        stress_reconnect: args.stress_reconnect,
    };
    let mut simulator = MockSimulator::new(sim_cfg);

    let reader = RfcommReader::new(1000);
    let handle = reader.spawn_from_generator(move || simulator.next_frame());

    let mut parser = PacketParser::new(None);
    let mut csv = CsvWriter::create(&args.csv)?;

    for _ in 0..500 {
        if let Some(frame) = reader.ring.pop_frame_timeout(Duration::from_millis(20)) {
            if let Ok(packet) = parser.parse_packet(&frame) {
                csv.write_packet(&packet)?;
            }
        }
    }

    reader.stop();
    let _ = handle.join();
    Ok(())
}
