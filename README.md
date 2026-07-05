# Neurable MW75 — Steam Deck / Jetson Orin EEG Pipeline

Production-ready Rust system for raw 12-channel EEG acquisition from Neurable MW75 Neuro headphones on Steam Deck (SteamOS/Linux) and Jetson Orin Nano (JetPack/Linux).

## Hardware

| Component | Details |
|-----------|---------|
| **EEG Headphones** | Neurable MW75 Neuro (12-channel dry EEG, 500 Hz) |
| **Bluetooth Adapter** | ASUS USB-BT500 (Bluetooth 5.0, backward compatible) |
| **Platform A** | Steam Deck (SteamOS 3.x / Arch Linux) |
| **Platform B** | Jetson Orin Nano (JetPack 6.x / Linux ARM64) |
| **BLE Stack** | BlueZ ≥ 5.44 |

---

## Features

- **12-channel raw EEG @ 500 Hz** via RFCOMM (channel 25)
- **BLE activation handshake** (ENABLE_EEG → ENABLE_RAW_MODE → BATTERY_CMD)
- **ADC → µV conversion** (scaling: × 0.023842)
- **Packet validation** – SOF check, checksum (u16 LE), frame synchroniser
- **Multiple outputs**: CSV, WebSocket, LSL (Lab Streaming Layer)
- **Real-time TUI** – 4-channel waveform viewer, battery display, pause/resume
- **Simulation mode** – synthetic EEG without hardware (test & demo)
- **Cross-platform**: Linux x86_64, ARM64 (Jetson), ARM (Steam Deck)

---

## Quick Start

### Prerequisites

```bash
# Arch / SteamOS
sudo pacman -S bluez bluez-utils rust

# Debian / Ubuntu / JetPack
sudo apt-get install bluez bluez-tools build-essential
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### BlueZ Setup

```bash
sudo bash scripts/setup-bluez.sh
```

### Pair MW75

```bash
bash scripts/pair-mw75.sh                      # interactive
bash scripts/pair-mw75.sh AA:BB:CC:DD:EE:FF   # direct
```

### Build

```bash
# Library + headless binaries (minimal)
cargo build --release --no-default-features

# With TUI (default)
cargo build --release

# With hardware RFCOMM support
cargo build --release --features rfcomm

# Full feature set
cargo build --release --all-features
```

### Run

```bash
# Simulation mode (no hardware)
cargo run --release -- --simulate

# Hardware streaming to CSV
cargo run --release -- --address AA:BB:CC:DD:EE:FF --csv eeg_data.csv

# TUI waveform viewer (simulation)
cargo run --release --bin eeg-tui -- --simulate

# Headless streamer
cargo run --release --bin eeg-stream -- --simulate -o output.csv

# BLE device scanner
cargo run --release --bin ble-probe -- --timeout 10

# RFCOMM protocol debugger (requires rfcomm feature)
cargo run --release --features rfcomm --bin rfcomm-debug -- --address AA:BB:CC:DD:EE:FF
```

---

## Project Structure

```
eeg-neurable-steamdeck/
├── Cargo.toml                  # Multi-platform, feature-gated deps
├── src/
│   ├── lib.rs                  # Public API
│   ├── main.rs                 # Headless daemon entry point
│   ├── protocol.rs             # GATT UUIDs, BLE commands, constants
│   ├── types.rs                # EegPacket, Mw75Event, BatteryInfo
│   ├── parse.rs                # Packet parsing, checksum, frame sync
│   ├── mw75_client.rs          # BLE scan/connect/activate
│   ├── rfcomm.rs               # RFCOMM transport (Linux bluer)
│   ├── simulate.rs             # Mock EEG generator
│   ├── output.rs               # CSV / WebSocket / LSL writers
│   └── logging.rs              # Env-controlled logging
├── src/bin/
│   ├── eeg_stream.rs           # Headless CSV/WS/LSL streamer
│   ├── eeg_tui.rs              # Real-time terminal viewer
│   ├── ble_probe.rs            # BLE discovery & GATT enum
│   └── rfcomm_debug.rs         # Protocol debugging
├── examples/
│   ├── basic_stream.rs
│   └── csv_output.rs
├── config/
│   ├── steamdeck.toml          # Steam Deck profile
│   └── jetson_orin.toml        # Jetson Orin profile
└── scripts/
    ├── setup-bluez.sh          # BlueZ install & config
    ├── pair-mw75.sh            # MW75 pairing helper
    └── cross-build.sh          # Cross-compile for ARM/ARM64
```

---

## Protocol Details

### BLE Activation Sequence

```
ENABLE_EEG  →  wait 100 ms  →  ENABLE_RAW_MODE  →  wait 500 ms  →  BATTERY_CMD
```

### RFCOMM Packet Format (63 bytes)

| Offset | Size | Field      | Description                       |
|--------|------|------------|-----------------------------------|
| 0      | 1    | SOF        | `0xAA` start-of-frame             |
| 1      | 1    | EVENT_ID   | `0x01` for EEG, `0x09` for battery|
| 2      | 1    | LEN        | Payload length                    |
| 3      | 1    | COUNTER    | Rolling packet counter (0–255)    |
| 4      | 4    | REF        | Reference electrode (f32 BE)      |
| 8      | 4    | DRL        | Driven Right Leg (f32 BE)         |
| 12     | 48   | EEG[0–11]  | 12 × f32 BE channel samples       |
| 60     | 1    | STATUS     | Signal quality flags              |
| 61     | 2    | CHECKSUM   | u16 LE checksum (sum of bytes 0–60)|

**Scaling**: `µV = raw_adc × 0.023842`

---

## Cargo Features

| Feature      | Description                                      | Extra deps              |
|--------------|--------------------------------------------------|-------------------------|
| `tui`        | Real-time terminal waveform viewer *(default)*   | ratatui, crossterm      |
| `rfcomm`     | RFCOMM streaming (Linux BlueZ only)              | bluer                   |
| `websocket`  | WebSocket broadcast server                       | tokio-tungstenite       |
| `lsl`        | Lab Streaming Layer outlet                       | lsl                     |
| `simulation` | Mock EEG generation (always compiled)            | —                       |

```bash
# Library only (no UI or platform deps)
cargo build --no-default-features

# Headless edge deployment
cargo build --no-default-features --bin eeg-stream

# Full hardware support + all outputs
cargo build --all-features
```

---

## Cross-Compilation

```bash
# Install targets
rustup target add aarch64-unknown-linux-gnu    # Jetson Orin
rustup target add armv7-unknown-linux-gnueabihf # Steam Deck

# Install cross-linkers (Ubuntu/Debian)
sudo apt-get install gcc-aarch64-linux-gnu gcc-arm-linux-gnueabihf

# Build all targets
bash scripts/cross-build.sh
```

---

## Configuration

Copy and adapt the appropriate profile from `config/`:

```bash
# Steam Deck
cp config/steamdeck.toml ~/.config/eeg-neurable/config.toml

# Jetson Orin
cp config/jetson_orin.toml /etc/eeg-neurable/config.toml
```

---

## Running Tests

```bash
cargo test                                # all tests
cargo test --no-default-features          # library tests only
cargo test parse                          # parse module tests
```

---

## Troubleshooting

**`bluetoothctl: No default controller`**
```bash
sudo systemctl start bluetooth
sudo rfkill unblock bluetooth
```

**`RFCOMM connect failed: Connection refused`**
- Ensure MW75 is paired and trusted: `bluetoothctl trust <MAC>`
- The MW75 must be powered on and in range
- Try: `bluetoothctl connect <MAC>` first

**`Permission denied on /dev/rfcomm0`**
```bash
sudo usermod -aG bluetooth $USER
# Log out and back in
```

**BlueZ version too old**
```bash
bluetoothctl --version   # need >= 5.44
# Upgrade BlueZ or use an alternative distribution
```

**Steam Deck: Bluetooth not scanning**
```bash
# Disable power management for the BT adapter
sudo btmgmt power on
```

---

## License

MIT

