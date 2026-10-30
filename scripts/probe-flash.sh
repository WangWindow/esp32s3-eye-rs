#!/usr/bin/env bash
# Flash only the application image with probe-rs.
#
# The first-stage/ESP-IDF bootloader and the partition table must already be on
# the board (scripts/flash.sh or one `espflash flash` run does this). After
# that, probe-rs can replace the application via the `idf` binary format.
set -euo pipefail

ELF="${1:?usage: probe-flash.sh <elf>}"

probe-rs download \
    --chip esp32s3 \
    --binary-format idf \
    "$ELF"

probe-rs reset --chip esp32s3
