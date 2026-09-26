#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
if ! command -v cargo >/dev/null; then
  echo "Rustをインストールしてください。手順はreadme.mdにあります。" >&2
  exit 1
fi
if [ ! -e amitoki.toml ]; then cp amitoki.example.toml amitoki.toml; fi
cargo build --release --locked
echo "amitoki.tomlとDB接続情報を設定してください。起動手順はreadme.mdにあります。"
