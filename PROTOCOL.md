# MW75 Neuro Protocol Reference

## Bluetooth UUIDs

| Role | UUID | Notes |
|---|---|---|
| GAIA Service | `00001100-d102-11e1-9b23-00025b00a5a5` | MW75 vendor-specific |
| GAIA Control Char | `00001101-d102-11e1-9b23-00025b00a5a5` | Write GAIA commands here |
| EEG Service | `df21fe2c-2515-4fdb-8886-f12c4d679277` | Raw 12-channel EEG |

> **These UUIDs are MW75-vendor-specific.** Generic Bluetooth UUIDs (fe59/fe5a/fe5b) will *not* connect to the headphones.

---

## GAIA Activation Sequence

All GAIA commands are **5-byte** writes to `GAIA_CONTROL_UUID` using GATT `WriteWithResponse`:

```
[vendor_id_hi, vendor_id_lo, msg_type, cmd, param]
```

| Command | Bytes | Delay after |
|---|---|---|
| `ENABLE_EEG_CMD` | `0x02 0x01 0x00 0x01 0x00` | 100 ms |
| `ENABLE_RAW_MODE_CMD` | `0x02 0x01 0x00 0x02 0x01` | 500 ms |
| `BATTERY_CMD` | `0x02 0x02 0x00 0x00 0x00` | — |

After `ENABLE_RAW_MODE_CMD` the device begins streaming 63-byte EEG frames on **RFCOMM channel 25**.

---

## RFCOMM Channels

| Channel | Purpose |
|---|---|
| 1 | Heartbeat / keep-alive (1 Hz pulse) |
| 25 | EEG data stream (500 Hz, 63-byte frames) |

---

## EEG Frame Layout (63 bytes, all multi-byte fields **little-endian**)

```
Offset | Size | Type    | Field
-------|------|---------|------------------------------------------
  0-1  |  2   | u8      | SOF marker: 0xA0, 0xA0
  2    |  1   | u8      | Reserved / padding
  3    |  1   | u8      | Rolling frame counter (wraps at 256)
  4-7  |  4   | f32 LE  | REF electrode voltage (μV)
  8-11 |  4   | f32 LE  | DRL electrode voltage (μV)
 12-59 | 48   | f32 LE  | 12 × EEG channel voltages (μV), packed
 60    |  1   | u8      | Device status flags
 61-62 |  2   | u16 LE  | Checksum (sum of bytes 0..61 mod 2^16)
```

> **Important:** All voltage fields use **little-endian** byte order. Using big-endian will produce incorrect values.

### Status Byte Flags

| Bit | Meaning |
|---|---|
| 0 | EEG active |
| 1 | Lead-off detected |
| 2-7 | Reserved |

### Checksum

```
checksum = sum(bytes[0..61]) mod 65536  (16-bit wrapping add)
```

---

## Timing

| Event | Timing |
|---|---|
| EEG sample rate | 500 Hz (2 ms per frame) |
| RFCOMM heartbeat | 1 Hz on channel 1 |
| GAIA session timeout | ~60 s without activity |
| Heartbeat absence timeout | 5 s → trigger reconnect |

---

## Build Variants

```sh
# Library only (no UI, no hardware)
cargo build --no-default-features

# Headless + hardware
cargo build --no-default-features --features rfcomm

# Full TUI + hardware
cargo build --features rfcomm

# All features
cargo build --all-features

# Mock testing (no hardware required)
cargo run --bin eeg-stream --features simulation -- --mock --drop-rate 0.05
```
