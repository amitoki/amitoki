#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

artifacts=$(cargo test --test linux_network --test block_process --locked --no-run --message-format=json)
mapfile -t test_binaries < <(python3 -c '
import json, sys
for line in sys.stdin:
    artifact = json.loads(line)
    if artifact.get("reason") == "compiler-artifact" and artifact.get("executable"):
        if artifact["target"]["name"] in ("linux_network", "block_process"):
            print(artifact["executable"])
' <<< "$artifacts")
[[ ${#test_binaries[@]} == 2 ]]
for index in "${!test_binaries[@]}"; do
  test_binaries[$index]="/work/${test_binaries[$index]#"$PWD"/}"
done
docker build --tag amitoki-network-test:local --file tests/network.Dockerfile .
# ホストのネットワークに接続せず、veth転送とプラグインへの権限継承を検証する。
docker run --rm --network none --cap-add NET_ADMIN --cap-add NET_RAW \
  --mount "type=bind,src=$PWD,dst=/work,readonly" \
  --env AMITOKI_TEST_BLOCK=/work/target/debug/amitoki-test-block \
  amitoki-network-test:local sh -eu -c '
    for node in a b c; do
      ip link add "relay-$node" type veth peer name "host-$node"
      ip link set "relay-$node" up
      ip link set "host-$node" up
    done
    for test_binary do
      "$test_binary" --ignored --nocapture
    done
  ' sh "${test_binaries[@]}"
