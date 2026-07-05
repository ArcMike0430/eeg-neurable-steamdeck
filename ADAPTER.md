# Bluetooth Adapter Guide

## Why USB-BT500?

The Jetson Orin Nano's **internal Realtek Bluetooth adapter** has a broken BLE
stack on Linux: GATT write operations return `ATT error 0x81` (mapped to ENOSYS
at the kernel layer).  This makes it impossible to write GAIA commands and
activate EEG streaming.

The **ASUS USB-BT500** (RTL8761B chipset + `btusb` kernel driver) provides a
fully functional BLE stack on Linux that behaves identically to **macOS
CoreBluetooth**: GATT reads/writes complete successfully and standard `btleplug`
code works without any workarounds.

---

## Hardware Setup

### Steam Deck

```
Steam Deck internal BT  →  hci0  (functional, works fine)
USB-BT500 dongle        →  hci1  (preferred for consistency)
```

Both adapters work on the Steam Deck.  USB-BT500 is preferred by `adapter.rs`
because it is guaranteed to work on both Steam Deck and Jetson.

### Jetson Orin Nano

```
Jetson internal BT      →  hci0  ❌ ATT 0x81 on BLE writes — do NOT use
USB-BT500 dongle        →  hci1  ✅ Required for EEG activation
```

**You MUST plug in the USB-BT500 before running eeg-stream on Jetson.**

---

## Driver Setup (Jetson)

```sh
# Check kernel module
lsmod | grep btusb

# Should list RTL8761B in hciconfig
hciconfig -a
# Look for:
#   hci1: Type: Primary  Bus: USB
#          Manufacturer: Realtek Semiconductor Corporation (93)

# If not detected, load the driver
sudo modprobe btusb
sudo hciconfig hci1 up
```

---

## Adapter Selection Logic

`src/adapter.rs` runs `hciconfig -a`, parses the output, and:

1. **Prefers USB adapters** (Bus: USB) with a Realtek manufacturer string.
2. Falls back to the first available adapter with a warning.
3. On Jetson, if only the internal adapter is available, logs a clear error:
   > *"On Jetson Orin the internal Realtek adapter returns ATT 0x81 (ENOSYS) on BLE GATT writes — EEG activation will fail. Plug in a USB-BT500 dongle and retry."*

---

## Overriding Adapter Selection

In `config/jetson-orin.toml`:

```toml
[device]
preferred_adapter = "hci1"

[ble]
adapter_index = 1
```

The `adapter_index` is passed to `btleplug` (0-based index into the adapter
list returned by `manager.adapters()`).

---

## Troubleshooting ATT 0x81

If you see:
```
GAIA ENABLE_EEG write failed. ATT error 0x81 (ENOSYS)
```

1. Check which adapter is selected: `hciconfig -a` and look for Bus type.
2. Ensure the USB-BT500 is plugged in.
3. Set `adapter_index = 1` (or whichever index is the USB adapter) in config.
4. Run `sudo hciconfig hci1 up` if the dongle is not UP.
