#!/usr/bin/env bash
set -euo pipefail

echo "Installing BlueZ and btusb support"
sudo apt-get update
sudo apt-get install -y bluez bluez-tools libdbus-1-dev

echo "Reloading btusb"
sudo modprobe -r btusb || true
sudo modprobe btusb

echo "Done. Verify with: ./scripts/detect-adapter.sh"
