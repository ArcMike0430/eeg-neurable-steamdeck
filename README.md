# EEG Raw Data Acquisition System for MW75

This repository provides a Rust-based EEG pipeline for Neurable MW75 Neuro headphones (12-channel dry EEG) on Steam Deck and Jetson Orin.

## Architecture
- **Two phase protocol:** BLE GAIA activation then BT Classic RFCOMM streaming (ch25, 63-byte packets at 500Hz).
- **Adapter preference:** USB Bluetooth adapters (USB-BT500 / btusb) are preferred and logged.
- **Peer topology:** Steam Deck and Jetson are independent peers with optional UDP multicast sync (`224.0.0.1:5005`).
- **Resilience:** bounded RFCOMM ring buffer, stall detection (>50ms), checksum validation, reconnect backoff.

## Quick start
```bash
cargo build --no-default-features
cargo build --features rfcomm
cargo run --bin eeg-stream --features simulation -- --mock --drop-rate 0.05 --jitter 25 --corrupt 0.01
```

## Jetson note
If BLE GAIA activation fails on the internal Realtek adapter (ATT 0x81 / ENOSYS), plug in USB-BT500 and rerun.

See `PROTOCOL.md`, `ADAPTER.md`, `PEER_SYNC.md`, `BUILD.md`, and `TROUBLESHOOTING.md`.
