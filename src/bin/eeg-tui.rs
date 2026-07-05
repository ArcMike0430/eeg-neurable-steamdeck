use eeg_neurable_steamdeck::adapter::enumerate_adapters;
use eeg_neurable_steamdeck::logging::init_logging;

fn main() {
    init_logging();
    println!("eeg-tui placeholder: adapters");
    for adapter in enumerate_adapters() {
        println!(
            "- {} usb={} driver={:?}",
            adapter.name, adapter.is_usb, adapter.driver
        );
    }
}
