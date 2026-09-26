#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

test_binary=$(cargo test --test linux_network --locked --no-run --message-format=json | python3 -c '
import json, sys
for line in sys.stdin:
    artifact = json.loads(line)
    if artifact.get("reason") == "compiler-artifact" and artifact.get("executable"):
        if artifact["target"]["name"] == "linux_network":
            print(artifact["executable"])
')
docker build --tag amitoki-network-test:local --file tests/network.Dockerfile .
# ホストのネットワークに接続せず、テスト用vethだけで実フレームを送受信する。
docker run --rm --network none --cap-add NET_ADMIN --cap-add NET_RAW \
  --mount "type=bind,src=$PWD,dst=/work,readonly" amitoki-network-test:local \
  sh -eu -c '
    for node in a b c; do
      ip link add "relay-$node" type veth peer name "host-$node"
      ip link set "relay-$node" up
      ip link set "host-$node" up
    done
    exec "$1" --ignored --nocapture
  ' sh "/work/${test_binary#"$PWD"/}"
