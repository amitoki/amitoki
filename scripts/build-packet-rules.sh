#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p amitoki-block-packet-rules --locked
python3 scripts/package-plugin.py target/release/amitoki-plugin-packet-rules dist/packet-rules
