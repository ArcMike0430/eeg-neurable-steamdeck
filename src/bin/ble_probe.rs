//! `ble-probe` — scan for Bluetooth adapters and nearby MW75 peripherals.
//!
//! Useful for diagnosing adapter selection, confirming USB-BT500 is detected,
//! and verifying the MW75 advertises the expected GAIA service UUID.
//!
//! # Usage
//! ```sh
//! cargo run --bin ble-probe
//! ```

use anyhow::Result;
use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::Manager;
use eeg_neurable_steamdeck::{adapter, init_logger, protocol};
use log::info;
use std::time::Duration;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    init_logger("info");

    info!("=== ble-probe: Bluetooth adapter scan ===");

    // ---- Enumerate adapters via hciconfig -----------------------------------
    let adapters = adapter::enumerate_adapters();
    if adapters.is_empty() {
        info!("No adapters found via hciconfig (normal in CI/VM environments)");
    } else {
        info!("Adapters found:");
        for a in &adapters {
            info!(
                "  {} — USB={} manufacturer={}",
                a.name,
                a.is_usb,
                a.manufacturer.as_deref().unwrap_or("<unknown>")
            );
        }
        if let Some(sel) = adapter::prefer_usb_adapter(&adapters) {
            info!("Selected adapter: {}", sel.name);
        }
    }

    // ---- BLE scan via btleplug ---------------------------------------------
    info!("=== btleplug BLE scan (10s) ===");

    let gaia_uuid = Uuid::parse_str(protocol::GAIA_SERVICE_UUID)?;

    let manager = match Manager::new().await {
        Ok(m) => m,
        Err(e) => {
            info!("btleplug Manager::new failed (expected in CI): {e}");
            info!("To use real hardware, ensure BlueZ is running and a BT adapter is present.");
            return Ok(());
        }
    };

    let ble_adapters = manager.adapters().await?;
    if ble_adapters.is_empty() {
        info!("No BLE adapters available. Plug in USB-BT500 dongle.");
        return Ok(());
    }

    info!("BLE adapters (btleplug): {}", ble_adapters.len());
    for (i, adapter) in ble_adapters.iter().enumerate() {
        info!("  [{}] {adapter:?}", i);

        if let Err(e) = adapter.start_scan(ScanFilter::default()).await {
            info!("  start_scan failed: {e}");
            continue;
        }

        tokio::time::sleep(Duration::from_secs(10)).await;

        let peripherals = adapter.peripherals().await?;
        info!("  Peripherals found: {}", peripherals.len());

        for p in &peripherals {
            let props = p.properties().await?;
            if let Some(props) = props {
                let name = props.local_name.as_deref().unwrap_or("<unnamed>");
                let is_mw75 = props.services.contains(&gaia_uuid)
                    || name.to_ascii_lowercase().contains("mw75");
                info!(
                    "    {} — RSSI={:?} services={} {}",
                    name,
                    props.rssi,
                    props.services.len(),
                    if is_mw75 { "⟵ MW75 FOUND" } else { "" }
                );
            }
        }
    }

    Ok(())
}
