//! BLE client for the Neurable MW75 Neuro headphones.
//!
//! Uses `btleplug` for all BLE operations.  With the USB-BT500 (RTL8761B +
//! `btusb` driver) the BLE stack behaves like macOS CoreBluetooth — standard
//! GATT writes complete without error and no ATT 0x81 workaround is required.
//!
//! # Activation sequence
//! 1. Scan for a peripheral advertising the GAIA service UUID.
//! 2. Connect and discover services.
//! 3. Write `ENABLE_EEG_CMD` → wait 100 ms.
//! 4. Write `ENABLE_RAW_MODE_CMD` → wait 500 ms.
//! 5. Optionally write `BATTERY_CMD` for a battery level query.
//!
//! After step 4 the device begins streaming 63-byte frames on RFCOMM ch 25.

use crate::protocol::{
    BATTERY_CMD, ENABLE_EEG_CMD, ENABLE_RAW_MODE_CMD, GAIA_CONTROL_UUID, GAIA_SERVICE_UUID,
    GAIA_ENABLE_DELAY_MS, GAIA_RAW_MODE_DELAY_MS, RECONNECT_BACKOFF_MAX_SECS,
};
use anyhow::{anyhow, Context, Result};
use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Manager};
use log::{debug, error, info, warn};
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

// Re-export so callers can hold a peripheral without importing btleplug directly.
pub use btleplug::platform::Peripheral;

/// Scan for an MW75 peripheral and return it (connected, services discovered).
///
/// If `adapter` is `Some`, that adapter index (0-based) is used.  Otherwise
/// the first available adapter is tried.
pub async fn find_and_connect(adapter_index: Option<usize>) -> Result<Peripheral> {
    let manager = Manager::new().await.context("BLE Manager::new")?;
    let adapters = manager.adapters().await.context("list BLE adapters")?;

    if adapters.is_empty() {
        return Err(anyhow!(
            "No Bluetooth adapters found. Plug in the USB-BT500 dongle."
        ));
    }

    let idx = adapter_index.unwrap_or(0);
    let adapter: &Adapter = adapters
        .get(idx)
        .ok_or_else(|| anyhow!("Adapter index {} out of range (found {})", idx, adapters.len()))?;

    info!("Starting BLE scan on adapter {}", idx);
    adapter
        .start_scan(ScanFilter::default())
        .await
        .context("start BLE scan")?;

    // Scan for up to 10 s
    let scan_deadline = std::time::Instant::now() + Duration::from_secs(10);
    let gaia_uuid = Uuid::parse_str(GAIA_SERVICE_UUID).expect("valid UUID constant");

    loop {
        if std::time::Instant::now() > scan_deadline {
            return Err(anyhow!(
                "MW75 not found within scan window. \
                 Ensure the headphones are on and pairable."
            ));
        }

        let peripherals = adapter
            .peripherals()
            .await
            .context("list peripherals")?;

        for p in peripherals {
            let props = p.properties().await.context("get peripheral props")?;
            if let Some(props) = props {
                let name = props.local_name.as_deref().unwrap_or("<unnamed>");
                debug!("Found peripheral: {name}");
                if props.services.contains(&gaia_uuid)
                    || name.to_ascii_lowercase().contains("mw75")
                {
                    info!("MW75 peripheral found: {name}");
                    p.connect().await.context("BLE connect")?;
                    p.discover_services().await.context("discover services")?;
                    return Ok(p);
                }
            }
        }

        sleep(Duration::from_millis(500)).await;
    }
}

/// Write GAIA activation commands to the connected peripheral.
///
/// # Errors
/// Returns an error with context if service/characteristic lookup or any
/// GATT write fails.  With USB-BT500 these writes complete successfully;
/// if you see ATT error 0x81 you are using the broken internal Jetson
/// Realtek adapter — switch to USB-BT500.
pub async fn activate_gaia(peripheral: &Peripheral) -> Result<()> {
    let gaia_service = Uuid::parse_str(GAIA_SERVICE_UUID).unwrap();
    let gaia_ctrl = Uuid::parse_str(GAIA_CONTROL_UUID).unwrap();

    // Locate the GAIA control characteristic
    let characteristics = peripheral.characteristics();
    let ctrl_char = characteristics
        .iter()
        .find(|c| c.uuid == gaia_ctrl && c.service_uuid == gaia_service)
        .ok_or_else(|| {
            anyhow!(
                "GAIA control characteristic {} not found in service {}. \
                 Services discovered: {:?}",
                gaia_ctrl,
                gaia_service,
                peripheral.services()
            )
        })?;

    // Step 1 — Enable EEG front-end
    info!("GAIA: writing ENABLE_EEG_CMD ({} bytes)", ENABLE_EEG_CMD.len());
    peripheral
        .write(ctrl_char, &ENABLE_EEG_CMD, WriteType::WithResponse)
        .await
        .with_context(|| {
            "GAIA ENABLE_EEG write failed. \
             If error is ATT 0x81 (ENOSYS), use USB-BT500 dongle instead of internal adapter."
        })?;
    sleep(Duration::from_millis(GAIA_ENABLE_DELAY_MS)).await;

    // Step 2 — Enable raw 500 Hz mode
    info!(
        "GAIA: writing ENABLE_RAW_MODE_CMD ({} bytes)",
        ENABLE_RAW_MODE_CMD.len()
    );
    peripheral
        .write(ctrl_char, &ENABLE_RAW_MODE_CMD, WriteType::WithResponse)
        .await
        .with_context(|| {
            "GAIA ENABLE_RAW_MODE write failed."
        })?;
    sleep(Duration::from_millis(GAIA_RAW_MODE_DELAY_MS)).await;

    info!("GAIA activation complete — RFCOMM ch25 should now be streaming EEG data");
    Ok(())
}

/// Query battery level.  Best-effort — failures are logged but not propagated.
pub async fn query_battery(peripheral: &Peripheral) {
    let gaia_service = Uuid::parse_str(GAIA_SERVICE_UUID).unwrap();
    let gaia_ctrl = Uuid::parse_str(GAIA_CONTROL_UUID).unwrap();

    let characteristics = peripheral.characteristics();
    if let Some(ctrl_char) = characteristics
        .iter()
        .find(|c| c.uuid == gaia_ctrl && c.service_uuid == gaia_service)
    {
        match peripheral
            .write(ctrl_char, &BATTERY_CMD, WriteType::WithResponse)
            .await
        {
            Ok(()) => info!("GAIA: BATTERY_CMD sent"),
            Err(e) => warn!("GAIA: BATTERY_CMD failed: {e}"),
        }
    }
}

/// Reconnect loop with exponential backoff (1 s → 2 s → 4 s → … → 30 s max).
///
/// Calls `find_and_connect` then `activate_gaia` until both succeed.
pub async fn reconnect_with_backoff(adapter_index: Option<usize>) -> Result<Peripheral> {
    let max_backoff = RECONNECT_BACKOFF_MAX_SECS;
    let mut backoff = 1u64;

    loop {
        info!("Attempting BLE reconnect (next retry in {backoff}s if this fails)…");
        match find_and_connect(adapter_index).await {
            Ok(p) => match activate_gaia(&p).await {
                Ok(()) => {
                    info!("Reconnect successful");
                    return Ok(p);
                }
                Err(e) => {
                    error!("Reconnect: GAIA activation failed: {e:#}");
                }
            },
            Err(e) => {
                error!("Reconnect: BLE connect failed: {e:#}");
            }
        }

        sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}
