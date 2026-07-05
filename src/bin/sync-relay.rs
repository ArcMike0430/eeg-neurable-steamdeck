use std::time::Duration;

use eeg_neurable_steamdeck::logging::init_logging;
use eeg_neurable_steamdeck::sync_peer::PeerSync;

fn main() -> anyhow::Result<()> {
    init_logging();
    let peer = PeerSync::bind()?;
    println!("sync-relay listening on 224.0.0.1:5005");

    loop {
        if let Some(msg) = peer.recv(Duration::from_millis(500))? {
            println!(
                "device={} epoch={} ts={} checksum={}",
                msg.device_id, msg.epoch_num, msg.timestamp_us, msg.checksum
            );
        }
    }
}
