# TROUBLESHOOTING

- **ATT 0x81 / ENOSYS on GAIA write:** Internal Jetson Realtek BLE issue. Use USB-BT500 (`btusb`).
- **No packets for >50ms:** reader stall detector warns; inspect range/link quality.
- **Checksum failures:** packet corruption or wrong packet framing. Use `rfcomm-debug`.
- **Periodic stream drop (~60s):** run reconnect flow; GAIA activation timeout likely hit.
