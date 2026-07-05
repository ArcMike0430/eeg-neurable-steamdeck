use std::time::Duration;

use eeg_neurable_steamdeck::logging::init_logging;
use eeg_neurable_steamdeck::parse::PacketParser;
use eeg_neurable_steamdeck::rfcomm::RfcommReader;
use eeg_neurable_steamdeck::simulate::{MockSimulator, SimulationConfig};

fn main() {
    init_logging();
    let mut sim = MockSimulator::new(SimulationConfig::default());
    let reader = RfcommReader::new(500);
    let handle = reader.spawn_from_generator(move || sim.next_frame());
    let mut parser = PacketParser::new(None);

    for _ in 0..20 {
        if let Some(frame) = reader.ring.pop_frame_timeout(Duration::from_millis(40)) {
            match parser.parse_packet(&frame) {
                Ok(pkt) => println!("counter={} checksum=0x{:04X}", pkt.counter, pkt.checksum),
                Err(err) => println!("parse error: {err}"),
            }
        } else {
            println!("stall alert: no packet >40ms");
        }
    }

    reader.stop();
    let _ = handle.join();
}
