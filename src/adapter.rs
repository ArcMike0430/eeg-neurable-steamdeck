//! Bluetooth adapter selection — prefers USB-BT500 (RTL8761B) over the
//! Jetson internal adapter.
//!
//! The Jetson internal Realtek BLE stack returns ATT 0x81 (ENOSYS) on GATT
//! writes, making GAIA activation impossible.  The USB-BT500 with the
//! `btusb`/RTL8761B driver behaves like macOS CoreBluetooth: GATT reads/writes
//! complete successfully and standard `btleplug` code works without any
//! workarounds.

use log::{info, warn};
use std::process::Command;

/// Information about a local Bluetooth adapter.
#[derive(Debug, Clone)]
pub struct AdapterInfo {
    /// e.g. "hci0"
    pub name: String,
    /// Manufacturer string from hciconfig, if available.
    pub manufacturer: Option<String>,
    /// True if this looks like a USB adapter (bus type "USB").
    pub is_usb: bool,
}

impl AdapterInfo {
    /// Returns true if this adapter looks like an ASUS USB-BT500 or similar
    /// RTL8761B-based dongle (preferred over internal Realtek adapters).
    pub fn is_preferred_usb(&self) -> bool {
        self.is_usb
            && self
                .manufacturer
                .as_deref()
                .map(|m| {
                    m.to_ascii_lowercase().contains("realtek")
                        || m.to_ascii_lowercase().contains("usb")
                })
                .unwrap_or(true)
    }
}

/// Enumerate all Bluetooth adapters visible via `hciconfig`.
///
/// Falls back to a single unnamed entry if `hciconfig` is not available
/// (e.g. in CI / mock environments).
pub fn enumerate_adapters() -> Vec<AdapterInfo> {
    let output = Command::new("hciconfig").arg("-a").output();

    match output {
        Err(_) => {
            // hciconfig not available (CI / mock env)
            vec![AdapterInfo {
                name: "hci0".to_string(),
                manufacturer: None,
                is_usb: false,
            }]
        }
        Ok(out) => parse_hciconfig_output(&String::from_utf8_lossy(&out.stdout)),
    }
}

/// Select the best adapter: USB adapters first, then any adapter.
///
/// Logs which adapter was selected and warns if a non-USB adapter is chosen on
/// a Jetson system (internal Realtek → ATT 0x81 is expected; use USB-BT500).
pub fn prefer_usb_adapter(adapters: &[AdapterInfo]) -> Option<&AdapterInfo> {
    if adapters.is_empty() {
        return None;
    }

    // Prefer USB adapters
    if let Some(usb) = adapters.iter().find(|a| a.is_preferred_usb()) {
        info!(
            "Selected USB Bluetooth adapter '{}' (USB-BT500 / RTL8761B — macOS CoreBluetooth-like behaviour on Linux)",
            usb.name
        );
        return Some(usb);
    }

    // Fall back to the first adapter with a warning
    let fallback = &adapters[0];
    warn!(
        "No USB Bluetooth adapter found. Using '{}'. \
         On Jetson Orin the internal Realtek adapter returns ATT 0x81 (ENOSYS) \
         on BLE GATT writes — EEG activation will fail. \
         Plug in a USB-BT500 dongle and retry.",
        fallback.name
    );
    Some(fallback)
}

// ---- Internal ---------------------------------------------------------------

fn parse_hciconfig_output(output: &str) -> Vec<AdapterInfo> {
    let mut adapters = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_manufacturer: Option<String> = None;
    let mut current_is_usb = false;

    for line in output.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();

        // New adapter block: line starts with "hciN:" and is not indented
        if trimmed.starts_with("hci") && trimmed.contains(':') && !line.starts_with('\t') {
            if let Some(name) = current_name.take() {
                adapters.push(AdapterInfo {
                    name,
                    manufacturer: current_manufacturer.take(),
                    is_usb: current_is_usb,
                });
            }
            let name = trimmed.split(':').next().unwrap_or("hci0").to_string();
            current_name = Some(name);
            current_is_usb = false;
            current_manufacturer = None;
            // The adapter header line itself may contain bus/manufacturer info
            // (e.g. "hci1:	Type: Primary  Bus: USB"), so fall through to the
            // checks below rather than using else-if.
        }

        if lower.contains("bus: usb") {
            current_is_usb = true;
        }
        if lower.contains("manufacturer") {
            if let Some(val) = trimmed.splitn(2, ':').nth(1) {
                let v = val.trim().to_string();
                if !v.is_empty() {
                    current_manufacturer = Some(v);
                }
            }
        }
    }

    if let Some(name) = current_name.take() {
        adapters.push(AdapterInfo {
            name,
            manufacturer: current_manufacturer,
            is_usb: current_is_usb,
        });
    }

    adapters
}

// ---- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HCICONFIG: &str = r#"hci1:	Type: Primary  Bus: USB
	BD Address: AA:BB:CC:DD:EE:FF  ACL MTU: 1021:8  SCO MTU: 255:12
	UP RUNNING PSCAN
	RX bytes:1234 acl:56 sco:0 events:78 errors:0
	TX bytes:910 acl:11 sco:0 commands:12 errors:0
	Features: ...
	Manufacturer: Realtek Semiconductor Corporation (93)

hci0:	Type: Primary  Bus: UART
	BD Address: 11:22:33:44:55:66  ACL MTU: 1021:8  SCO MTU: 255:12
	UP RUNNING PSCAN
	Manufacturer: Realtek Semiconductor Corporation (93)
"#;

    #[test]
    fn test_parse_hciconfig() {
        let adapters = parse_hciconfig_output(SAMPLE_HCICONFIG);
        assert_eq!(adapters.len(), 2);

        let usb = adapters.iter().find(|a| a.name == "hci1").unwrap();
        assert!(usb.is_usb);

        let uart = adapters.iter().find(|a| a.name == "hci0").unwrap();
        assert!(!uart.is_usb);
    }

    #[test]
    fn test_prefer_usb_adapter() {
        let adapters = parse_hciconfig_output(SAMPLE_HCICONFIG);
        let selected = prefer_usb_adapter(&adapters).unwrap();
        assert_eq!(selected.name, "hci1");
        assert!(selected.is_usb);
    }

    #[test]
    fn test_prefer_usb_falls_back_to_first() {
        let adapters = vec![AdapterInfo {
            name: "hci0".to_string(),
            manufacturer: Some("Internal".to_string()),
            is_usb: false,
        }];
        let selected = prefer_usb_adapter(&adapters).unwrap();
        assert_eq!(selected.name, "hci0");
    }

    #[test]
    fn test_empty_adapters() {
        let selected = prefer_usb_adapter(&[]);
        assert!(selected.is_none());
    }
}
