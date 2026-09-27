#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../web"
if ! command -v npm >/dev/null; then
  echo "Node.js 24以降をインストールしてください。手順はreadme.mdにあります。" >&2
  exit 1
fi
npm ci
npm run build
