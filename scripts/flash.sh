#!/usr/bin/env bash
# Default cargo runner.
#
# The ESP32-S3-EYE has no USB-to-UART bridge: the ESP32-S3's built-in
# USB-Serial/JTAG is used for both flashing and the console. espflash knows how
# to provision the ESP-IDF second-stage bootloader and partition table, which a
# fresh board needs. After that, scripts/probe-flash.sh can update the app with
# probe-rs only.
set -euo pipefail

ELF="${1:?usage: flash.sh <elf>}"
PORT="${ESPFLASH_PORT:-/dev/ttyACM0}"

if command -v espflash >/dev/null 2>&1; then
    # Open the serial monitor only when cargo was started from a terminal.
    if [ -t 0 ]; then
        exec espflash flash \
            --chip esp32s3 \
            --port "$PORT" \
            --after watchdog-reset \
            --monitor \
            "$ELF"
    else
        exec espflash flash \
            --chip esp32s3 \
            --port "$PORT" \
            --after watchdog-reset \
            "$ELF"
    fi
fi

# Fallback for machines that only have probe-rs. This requires the bootloader
# and partition table to have been flashed once by espflash.
if command -v probe-rs >/dev/null 2>&1; then
    probe-rs download --chip esp32s3 --binary-format idf "$ELF"
    probe-rs reset --chip esp32s3
    exit 0
fi

echo "Neither espflash nor probe-rs is installed." >&2
echo "Install espflash with: cargo install espflash --locked" >&2
exit 1
