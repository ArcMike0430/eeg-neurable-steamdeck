# PROTOCOL

- GAIA control characteristic: `00001101-d102-11e1-9b23-00025b00a5a5`
- Activation sequence: `ENABLE_EEG (100ms) -> ENABLE_RAW_MODE (500ms) -> BATTERY_CMD`
- EEG stream: RFCOMM channel 25, packet size 63 bytes, 500Hz.
- Checksum: little-endian u16 additive checksum over bytes `[0..61)`.
- Scaling default: `0.023842` µV/ADC (configurable).
