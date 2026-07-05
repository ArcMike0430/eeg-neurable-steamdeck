use std::process::Command;

use crate::types::AdapterInfo;

const USB_INDICATORS: &[&str] = &["USB", "btusb", "RTL8761"];

pub fn enumerate_adapters() -> Vec<AdapterInfo> {
    let output = Command::new("hciconfig").arg("-a").output();
    let Ok(output) = output else {
        return vec![AdapterInfo {
            name: "mock-usb-bt500".to_string(),
            address: None,
            driver: Some("btusb".to_string()),
            is_usb: true,
        }];
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut adapters = Vec::new();
    for block in text.split("\n\n") {
        if let Some(first) = block.lines().next() {
            let name = first
                .split(':')
                .next()
                .unwrap_or("unknown")
                .trim()
                .to_string();
            if name.is_empty() {
                continue;
            }
            let is_usb = USB_INDICATORS.iter().any(|needle| block.contains(needle));
            let address = block
                .lines()
                .find(|line| line.contains("BD Address"))
                .and_then(|line| line.split_whitespace().nth(2))
                .map(ToString::to_string);

            adapters.push(AdapterInfo {
                name,
                address,
                driver: if is_usb { Some("btusb".into()) } else { None },
                is_usb,
            });
        }
    }

    adapters
}

pub fn prefer_usb_adapter(adapters: &[AdapterInfo]) -> Option<AdapterInfo> {
    adapters
        .iter()
        .find(|a| a.is_usb)
        .cloned()
        .or_else(|| adapters.first().cloned())
}
