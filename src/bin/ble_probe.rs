//! `ble-probe` – Bluetooth device discovery and GATT enumeration.
//!
//! Scans for nearby BLE devices and prints their names, addresses, and
//! available services/characteristics.
//!
//! ```bash
//! ble-probe --timeout 10
//! ble-probe --address AA:BB:CC:DD:EE:FF   # enumerate a specific device
//! ```

use anyhow::{Context, Result};
use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::Manager;
use clap::Parser;
use std::time::Duration;
use tokio::time::sleep;

use eeg_neurable_steamdeck::logging;

#[derive(Parser, Debug)]
#[command(
    name = "ble-probe",
    version,
    about = "BLE device discovery and GATT enumeration"
)]
struct Args {
    /// Scan duration in seconds
    #[arg(short, long, default_value_t = 10)]
    timeout: u64,

    /// Filter by name prefix (e.g. "MW75")
    #[arg(short, long)]
    name: Option<String>,

    /// Bluetooth address to enumerate (skip scan, connect directly)
    #[arg(short, long)]
    address: Option<String>,

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

    let manager = Manager::new().await.context("failed to create BLE manager")?;
    let adapters = manager.adapters().await.context("no BLE adapters")?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no BLE adapter found"))?;

    let adapter_info = adapter.adapter_info().await.unwrap_or_default();
    println!("BLE Adapter: {adapter_info}");
    println!("Scanning for {s}s…\n", s = args.timeout);

    adapter
        .start_scan(ScanFilter::default())
        .await
        .context("scan start failed")?;

    sleep(Duration::from_secs(args.timeout)).await;
    adapter.stop_scan().await.ok();

    let peripherals = adapter.peripherals().await.context("peripheral list failed")?;
    println!("Found {} devices:\n", peripherals.len());

    for p in &peripherals {
        let props = match p.properties().await {
            Ok(Some(pr)) => pr,
            _ => continue,
        };

        let name = props.local_name.clone().unwrap_or_else(|| "(unknown)".into());

        // Optional name filter
        if let Some(filter) = &args.name {
            if !name.to_lowercase().contains(&filter.to_lowercase()) {
                continue;
            }
        }

        let rssi = props.rssi.map(|r| format!("{r} dBm")).unwrap_or_else(|| "?".into());
        println!("  [{addr}] {name} (RSSI: {rssi})", addr = props.address);

        // If address match or enumerate-all, show services
        let should_enumerate = args
            .address
            .as_ref()
            .map(|a| props.address.to_string().eq_ignore_ascii_case(a))
            .unwrap_or(false);

        if should_enumerate {
            println!("  Connecting for GATT enumeration…");
            if p.connect().await.is_ok() {
                if p.discover_services().await.is_ok() {
                    for svc in p.services() {
                        println!("    Service: {}", svc.uuid);
                        for ch in &svc.characteristics {
                            println!(
                                "      Char: {}  props: {:?}",
                                ch.uuid, ch.properties
                            );
                        }
                    }
                }
                p.disconnect().await.ok();
            } else {
                println!("  (connection failed)");
            }
        }
    }

    Ok(())
}
