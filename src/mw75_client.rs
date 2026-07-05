use std::thread;

use anyhow::{bail, Result};
use log::{info, warn};

use crate::adapter::{enumerate_adapters, prefer_usb_adapter};
use crate::protocol::GAIA_ACTIVATION_SEQUENCE;
use crate::types::AdapterInfo;

pub struct Mw75Client {
    pub adapter: Option<AdapterInfo>,
    pub retries: usize,
}

impl Default for Mw75Client {
    fn default() -> Self {
        Self {
            adapter: None,
            retries: 6,
        }
    }
}

impl Mw75Client {
    pub fn select_adapter(&mut self) -> Result<AdapterInfo> {
        let adapters = enumerate_adapters();
        let selected = prefer_usb_adapter(&adapters)
            .ok_or_else(|| anyhow::anyhow!("no bluetooth adapters found"))?;
        if selected.is_usb {
            info!(
                "using USB adapter {} ({:?})",
                selected.name, selected.driver
            );
        } else {
            warn!(
                "using non-USB adapter {}. On Jetson internal Realtek, ATT 0x81 is expected; use USB-BT500",
                selected.name
            );
        }
        self.adapter = Some(selected.clone());
        Ok(selected)
    }

    pub fn activate_gaia(&self) -> Result<()> {
        for (cmd, delay) in GAIA_ACTIVATION_SEQUENCE {
            info!("sending GAIA cmd: {:02X?}", cmd);
            thread::sleep(delay);
        }
        Ok(())
    }

    pub fn reconnect_with_backoff(&self) -> Result<()> {
        for attempt in 0..self.retries {
            // Exponential backoff: 1s, 2s, 4s, ... (capped to 30s between retries).
            let wait = (1u64 << attempt.min(5)).min(30);
            info!("reconnect attempt {} in {}s", attempt + 1, wait);
            thread::sleep(std::time::Duration::from_secs(wait));
            if self.activate_gaia().is_ok() {
                info!("reconnect successful");
                return Ok(());
            }
        }
        bail!("reconnect failed after {} attempts", self.retries)
    }
}
