#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p amitoki-stage-telemetry-rewrite --locked
python3 scripts/package-plugin.py target/release/amitoki-plugin-telemetry-rewrite dist/telemetry-rewrite
