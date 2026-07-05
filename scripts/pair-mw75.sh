#!/usr/bin/env bash
# pair-mw75.sh – Interactive helper to pair and trust the Neurable MW75.
#
# Usage:
#   bash scripts/pair-mw75.sh
#   bash scripts/pair-mw75.sh AA:BB:CC:DD:EE:FF   # non-interactive

set -euo pipefail

GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info() { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC}  $*"; }

MAC="${1:-}"

if [[ -z "$MAC" ]]; then
    info "Scanning for MW75 devices (10s)…"
    bluetoothctl -- scan on &
    SCAN_PID=$!
    sleep 10
    kill $SCAN_PID 2>/dev/null || true

    echo
    info "Devices found:"
    bluetoothctl -- devices | grep -i "MW75" || warn "No MW75 found – check it is powered on and in range"
    echo
    read -rp "Enter MW75 Bluetooth address (AA:BB:CC:DD:EE:FF): " MAC
fi

info "Pairing $MAC…"
bluetoothctl -- pair    "$MAC"
bluetoothctl -- trust   "$MAC"
bluetoothctl -- connect "$MAC"

info "Done.  MW75 is paired, trusted, and connected."
info "Address saved. Use it with:  eeg-stream --address $MAC"
