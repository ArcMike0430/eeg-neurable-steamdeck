//! BLE scanning, connection, and activation sequence for Neurable MW75.
//!
//! Uses `btleplug` for cross-platform BLE support (Linux BlueZ ≥ 5.44,
//! macOS CoreBluetooth, Windows WinRT).

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use btleplug::api::{
    Central, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Manager, Peripheral};
use log::{debug, info};
use tokio::time::sleep;
use uuid::Uuid;

use crate::protocol::*;

// ── Public API ───────────────────────────────────────────────────────────────

/// Scan for and return the first MW75 peripheral found within `timeout`.
pub async fn discover_mw75(timeout: Duration) -> Result<Peripheral> {
    let manager = Manager::new().await.context("failed to create BLE manager")?;
    let adapters = manager.adapters().await.context("no BLE adapters found")?;
    let adapter = adapters.into_iter().next().ok_or_else(|| anyhow!("no BLE adapter available"))?;

    info!("Scanning for MW75 ({}s)…", timeout.as_secs());
    adapter
        .start_scan(ScanFilter::default())
        .await
        .context("failed to start BLE scan")?;

    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() >= deadline {
            adapter.stop_scan().await.ok();
            return Err(anyhow!("timed out – no MW75 found within {:?}", timeout));
        }

        let peripherals = adapter
            .peripherals()
            .await
            .context("failed to list peripherals")?;

        for p in peripherals {
            if let Ok(Some(props)) = p.properties().await {
                let name = props.local_name.unwrap_or_default();
                if name.starts_with(DEVICE_NAME_PREFIX) {
                    info!("Found MW75: {} ({})", name, props.address);
                    adapter.stop_scan().await.ok();
                    return Ok(p);
                }
            }
        }

        sleep(Duration::from_millis(200)).await;
    }
}

/// Connect to the peripheral and run the BLE activation sequence:
///   ENABLE_EEG → 100 ms → ENABLE_RAW_MODE → 500 ms → BATTERY_CMD
pub async fn activate_eeg(peripheral: &Peripheral) -> Result<()> {
    if !peripheral.is_connected().await? {
        peripheral.connect().await.context("BLE connect failed")?;
    }

    peripheral
        .discover_services()
        .await
        .context("service discovery failed")?;

    let cmd_char = find_characteristic(peripheral, &CMD_CHAR_UUID)
        .ok_or_else(|| anyhow!("command characteristic not found"))?;

    // Step 1: Enable EEG
    debug!("Sending ENABLE_EEG");
    peripheral
        .write(&cmd_char, ENABLE_EEG, WriteType::WithoutResponse)
        .await
        .context("ENABLE_EEG write failed")?;
    sleep(Duration::from_millis(EEG_ENABLE_DELAY_MS)).await;

    // Step 2: Enable raw mode
    debug!("Sending ENABLE_RAW_MODE");
    peripheral
        .write(&cmd_char, ENABLE_RAW_MODE, WriteType::WithoutResponse)
        .await
        .context("ENABLE_RAW_MODE write failed")?;
    sleep(Duration::from_millis(RAW_MODE_DELAY_MS)).await;

    // Step 3: Request battery status
    debug!("Sending BATTERY_CMD");
    peripheral
        .write(&cmd_char, BATTERY_CMD, WriteType::WithoutResponse)
        .await
        .context("BATTERY_CMD write failed")?;

    info!("BLE activation sequence complete");
    Ok(())
}

/// Subscribe to the notify characteristic so we receive BLE notifications.
pub async fn subscribe_notifications(peripheral: &Peripheral) -> Result<()> {
    let notify_char = find_characteristic(peripheral, &NOTIFY_CHAR_UUID)
        .ok_or_else(|| anyhow!("notify characteristic not found"))?;

    peripheral
        .subscribe(&notify_char)
        .await
        .context("failed to subscribe to notify characteristic")?;

    info!("Subscribed to BLE notifications");
    Ok(())
}

/// Gracefully stop EEG streaming and disconnect.
pub async fn deactivate_eeg(peripheral: &Peripheral) -> Result<()> {
    if !peripheral.is_connected().await? {
        return Ok(());
    }

    if let Some(cmd_char) = find_characteristic(peripheral, &CMD_CHAR_UUID) {
        peripheral
            .write(&cmd_char, DISABLE_EEG, WriteType::WithoutResponse)
            .await
            .ok();
    }

    peripheral.disconnect().await.context("BLE disconnect failed")?;
    info!("Disconnected from MW75");
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn find_characteristic(
    peripheral: &Peripheral,
    uuid: &Uuid,
) -> Option<btleplug::api::Characteristic> {
    peripheral.characteristics().into_iter().find(|c| &c.uuid == uuid)
}
