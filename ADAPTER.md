# ADAPTER

The code enumerates adapters and prefers USB-based devices (USB-BT500 class) over internal adapters.

Jetson internal Realtek BLE can fail GAIA writes with ATT 0x81 / ENOSYS. USB-BT500 with `btusb` is the recommended path.

Use `scripts/detect-adapter.sh` to inspect adapter and driver status.
