#!/usr/bin/env bash
# setup-bluez.sh – Install and configure BlueZ for MW75 EEG streaming
#
# Tested on:
#   - SteamOS 3.x (Arch Linux base)
#   - Ubuntu 22.04 / Jetson Orin JetPack 6.x
#   - Debian 12 (bookworm)
#
# Usage:
#   sudo bash scripts/setup-bluez.sh

set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

# ── Root check ────────────────────────────────────────────────────────────────
if [[ "$EUID" -ne 0 ]]; then
    error "This script must be run as root (sudo)."
    exit 1
fi

# ── Detect distro ─────────────────────────────────────────────────────────────
if command -v pacman &>/dev/null; then
    DISTRO="arch"
elif command -v apt-get &>/dev/null; then
    DISTRO="debian"
else
    warn "Unknown distro – attempting generic install"
    DISTRO="generic"
fi

# ── Install BlueZ ─────────────────────────────────────────────────────────────
info "Installing BlueZ ($DISTRO)…"
case "$DISTRO" in
    arch)
        pacman -Sy --noconfirm bluez bluez-utils
        ;;
    debian)
        apt-get update -qq
        apt-get install -y bluez bluez-tools
        ;;
    generic)
        warn "Manual install required: bluez >= 5.44"
        ;;
esac

# ── Enable and start bluetooth service ───────────────────────────────────────
info "Enabling bluetooth.service…"
systemctl enable bluetooth.service
systemctl start  bluetooth.service

# ── Verify BlueZ version ─────────────────────────────────────────────────────
BT_VERSION=$(bluetoothctl --version 2>/dev/null | awk '{print $2}' || echo "unknown")
info "BlueZ version: $BT_VERSION"

# Compare version (need >= 5.44)
BT_MAJOR=$(echo "$BT_VERSION" | cut -d. -f1)
BT_MINOR=$(echo "$BT_VERSION" | cut -d. -f2)
if [[ "$BT_MAJOR" -lt 5 ]] || { [[ "$BT_MAJOR" -eq 5 ]] && [[ "$BT_MINOR" -lt 44 ]]; }; then
    error "BlueZ $BT_VERSION is too old.  Need >= 5.44 for RFCOMM support."
    exit 1
fi

# ── RFCOMM kernel module ──────────────────────────────────────────────────────
info "Loading rfcomm kernel module…"
modprobe rfcomm || warn "rfcomm module not available (may already be built-in)"

# Persist across reboots
if [[ -d /etc/modules-load.d ]]; then
    echo "rfcomm" > /etc/modules-load.d/rfcomm.conf
fi

# ── Add user to bluetooth group ───────────────────────────────────────────────
if [[ -n "${SUDO_USER:-}" ]]; then
    info "Adding $SUDO_USER to the 'bluetooth' group…"
    usermod -aG bluetooth "$SUDO_USER"
    info "Log out and back in (or run 'newgrp bluetooth') for the group to take effect."
fi

# ── ASUS USB-BT500 udev rule ─────────────────────────────────────────────────
info "Installing udev rule for ASUS USB-BT500 (VID:0b05, PID:1939)…"
cat > /etc/udev/rules.d/99-asus-bt500.rules << 'UDEV'
# ASUS USB-BT500 Bluetooth 5.0 adapter
SUBSYSTEM=="usb", ATTRS{idVendor}=="0b05", ATTRS{idProduct}=="1939", \
    MODE="0666", GROUP="bluetooth"
UDEV

udevadm control --reload-rules
udevadm trigger

# ── RFCOMM user permissions ───────────────────────────────────────────────────
info "Setting RFCOMM device permissions…"
if [[ -c /dev/rfcomm0 ]]; then
    chmod 0660 /dev/rfcomm0
    chgrp bluetooth /dev/rfcomm0 2>/dev/null || true
fi

# ── Done ─────────────────────────────────────────────────────────────────────
echo
info "BlueZ setup complete."
info ""
info "Next steps:"
info "  1. Pair your MW75:  bluetoothctl → scan on → pair <MAC> → trust <MAC>"
info "  2. Run:             eeg-stream --address <MAC>"
info "  3. TUI viewer:      eeg-tui --address <MAC>"
info ""
