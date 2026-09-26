#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p amitoki-stage-telemetry --locked
python3 scripts/package-plugin.py target/release/amitoki-plugin-telemetry dist/telemetry
