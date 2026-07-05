# Troubleshooting Guide

## EEG Not Streaming

### Symptom: `MW75 not found within scan window`
- Check that headphones are powered on and in Bluetooth pairing mode.
- Verify the USB-BT500 is plugged in (`hciconfig -a`, look for Bus: USB).
- On Jetson, ensure `adapter_index = 1` in config (not the internal adapter).

### Symptom: `GAIA ENABLE_EEG write failed. ATT error 0x81 (ENOSYS)`
- You are using the **Jetson internal Realtek adapter**.
- Plug in the USB-BT500 dongle and set `adapter_index = 1`.
- See [ADAPTER.md](ADAPTER.md) for full setup instructions.

### Symptom: Activation succeeds but no EEG frames arrive
- Ensure RFCOMM channel 25 is accessible (`rfcomm connect <MAC> 25`).
- Verify `--features rfcomm` is included in your build.
- Check that BlueZ is running: `systemctl status bluetooth`.

---

## Keep-Alive / Reconnect

### Symptom: `no heartbeat for 5s (attempt N, reconnecting in Xs)`
- Normal: device GAIA session expired (60s timeout).
- Watchdog triggers auto-reconnect with exponential backoff.
- If reconnects keep failing, check Bluetooth signal strength.

### Symptom: Reconnect loops without success
- Ensure headphones are on and within range.
- Check for other devices holding the RFCOMM connection.
- Restart BlueZ: `sudo systemctl restart bluetooth`.

---

## RFCOMM Stall / Gaps

### Symptom: `RFCOMM stall detected: Xms since last EEG frame`
- Expected during reconnect events.
- If persistent, check USB bandwidth (other USB-heavy workloads on Steam Deck
  can briefly stall the USB-BT500).

### Symptom: Many `gaps_filled` or `gaps_lost` in stats
- Frame drops can occur due to RF interference or BLE congestion.
- `gaps_filled` (≤3 frames): linear interpolation applied — data is usable.
- `gaps_lost` (>3 frames): NaN values emitted — discard these samples.

---

## Peer Sync

### Symptom: `PeerSync: broadcast failed (graceful fallback)`
- The multicast group `224.0.0.1:5005` may not be routable on your LAN.
- Each device continues standalone acquisition; sync is best-effort.
- Verify both devices are on the same L2 network segment.

---

## Build Issues

### `error: could not find native library 'dbus-1'`
```sh
sudo apt-get install libdbus-1-dev pkg-config
```

### `RFCOMM feature not compiled in`
```sh
cargo build --features rfcomm
```

### Mock mode smoke test
```sh
cargo run --bin eeg-stream --features simulation -- --mock --timeout-after 100
# Should print 100 frame entries then exit cleanly
```

---

## Useful Diagnostics

```sh
# List all BT adapters
hciconfig -a

# Scan for MW75 device
hcitool scan
bluetoothctl scan on

# Probe BLE adapters and MW75 advertisement
cargo run --bin ble-probe

# Dump RFCOMM traffic (requires rfcomm tool)
sudo rfcomm connect 0 <MW75_MAC> 25

# Check BlueZ logs
journalctl -u bluetooth -f
```
