#!/usr/bin/env bash
set -euo pipefail

echo "=== Bluetooth adapters ==="
hciconfig -a || true

echo "=== USB Bluetooth devices ==="
lsusb | grep -Ei "bluetooth|rtl8761|asus" || true

echo "=== btusb kernel messages ==="
dmesg | grep -Ei "btusb|rtl8761|bluetooth" | tail -n 30 || true
