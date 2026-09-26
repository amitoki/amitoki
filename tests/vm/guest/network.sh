#!/usr/bin/env bash
set -euo pipefail
node=$(cat /opt/amitoki-lab/node)
if [[ ${1:-start} == stop ]]; then
  ip link delete relay0 2>/dev/null || true
  ip netns delete client 2>/dev/null || true
  exit 0
fi
case "$node" in a) address=11 ;; b) address=12 ;; c) address=13 ;; *) exit 1 ;; esac
ip netns add client
ip link add relay0 type veth peer name client0
ip link set client0 netns client
ip link set relay0 up
ip netns exec client ip link set lo up
ip netns exec client ip link set client0 address "02:00:00:00:00:$address"
ip netns exec client ip address add "192.0.2.$address/24" dev client0
ip netns exec client ip link set client0 up
# raw socket転送はchecksum/GSOのメタデータを運ばないため、実フレームとして完成させる。
ethtool -K relay0 gro off gso off tso off rx off tx off >/dev/null
ip netns exec client ethtool -K client0 gro off gso off tso off rx off tx off >/dev/null
sysctl -qw net.ipv6.conf.relay0.disable_ipv6=1
ip netns exec client sysctl -qw net.ipv6.conf.client0.disable_ipv6=1
