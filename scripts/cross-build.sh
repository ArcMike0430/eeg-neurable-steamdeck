#!/usr/bin/env bash
# cross-build.sh – Cross-compile for Steam Deck (ARM) and Jetson Orin (ARM64)
#
# Prerequisites:
#   rustup target add aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf
#   sudo apt-get install gcc-aarch64-linux-gnu gcc-arm-linux-gnueabihf
#
# Usage:
#   bash scripts/cross-build.sh

set -euo pipefail

GREEN='\033[0;32m'; NC='\033[0m'
info() { echo -e "${GREEN}[INFO]${NC}  $*"; }

FEATURES="${FEATURES:-}"   # e.g. FEATURES="rfcomm,websocket"

build_target() {
    local target="$1"
    local name="$2"
    info "Building for $name ($target)…"
    cargo build --release --target "$target" \
        ${FEATURES:+--features "$FEATURES"} \
        2>&1 | tail -5
    info "$name build complete → target/$target/release/"
}

# ── x86_64 (native host) ─────────────────────────────────────────────────────
info "Building for native x86_64…"
cargo build --release ${FEATURES:+--features "$FEATURES"}

# ── ARM64 – Jetson Orin Nano ─────────────────────────────────────────────────
if command -v aarch64-linux-gnu-gcc &>/dev/null; then
    build_target "aarch64-unknown-linux-gnu" "Jetson Orin (ARM64)"
else
    info "Skipping ARM64 build: aarch64-linux-gnu-gcc not found"
fi

# ── ARMv7 – Steam Deck SteamOS 3.x ───────────────────────────────────────────
if command -v arm-linux-gnueabihf-gcc &>/dev/null; then
    build_target "armv7-unknown-linux-gnueabihf" "Steam Deck (ARMv7)"
else
    info "Skipping ARMv7 build: arm-linux-gnueabihf-gcc not found"
fi

info ""
info "Build summary:"
find target -maxdepth 3 -name "eeg-*" -o -name "ble-probe" -o -name "rfcomm-debug" \
    2>/dev/null | grep -v '\.d$' | sort | while read -r f; do
    info "  $f  ($(du -sh "$f" 2>/dev/null | cut -f1))"
done
