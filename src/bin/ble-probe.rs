use eeg_neurable_steamdeck::adapter::{enumerate_adapters, prefer_usb_adapter};
use eeg_neurable_steamdeck::logging::init_logging;
use eeg_neurable_steamdeck::protocol::{EEG_SERVICE_UUID, GAIA_CONTROL_UUID, GAIA_SERVICE_UUID};

fn main() {
    init_logging();
    println!("GAIA service UUID: {GAIA_SERVICE_UUID}");
    println!("GAIA control UUID: {GAIA_CONTROL_UUID}");
    println!("EEG service UUID: {EEG_SERVICE_UUID}");

    let adapters = enumerate_adapters();
    for adapter in &adapters {
        println!(
            "adapter={} usb={} addr={:?}",
            adapter.name, adapter.is_usb, adapter.address
        );
    }
    if let Some(selected) = prefer_usb_adapter(&adapters) {
        println!("preferred adapter: {}", selected.name);
    }
}
