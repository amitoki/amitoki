#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
git submodule update --init plugins/l3
cargo build --manifest-path plugins/l3/Cargo.toml --release -p amitoki-plugin-l3 --locked
python3 scripts/package-plugin.py plugins/l3/target/release/amitoki-plugin-l3 dist/l3
# 補助プロセスは本体のCLIから自動起動せず、利用者が明示して起動する。
cp plugins/l3/target/release/amitoki-l3-broker dist/l3/amitoki-l3-broker
