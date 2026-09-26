#!/usr/bin/env python3
"""PCAPデバッグ用の、通過・破棄・不正長を含む合成Ethernetフレームを保存する。"""
import argparse
from pathlib import Path
import struct

# 実験用EtherType 0x88b5/0x88b6。実ネットワークへは送信しない。
BLOCKED_ETHER_TYPE = 0x88B5
ALLOWED_ETHER_TYPE = 0x88B6
MAX_FRAME_BYTES = 65535
LINKTYPE_ETHERNET = 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    arguments = parser.parse_args()
    header = bytes.fromhex("020000000012020000000011")
    packets = [header + struct.pack("!H", ether_type) + b"amitoki-debug"
               for ether_type in (ALLOWED_ETHER_TYPE, BLOCKED_ETHER_TYPE)]
    packets.append(header)
    # 意図せず既存のキャプチャを上書きしない。
    with arguments.destination.open("xb") as stream:
        stream.write(struct.pack("<IHHIIII", 0xA1B2C3D4, 2, 4, 0, 0, MAX_FRAME_BYTES, LINKTYPE_ETHERNET))
        for index, packet in enumerate(packets):
            stream.write(struct.pack("<IIII", index, 0, len(packet), len(packet)))
            stream.write(packet)
    print(arguments.destination)


if __name__ == "__main__":
    main()
