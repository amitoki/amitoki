"""実験用EtherTypeの到達数を数え、TCP/UDPの重複吸収に隠れた再注入を調べる。"""
import json
from pathlib import Path
import socket
import threading

# IEEE実験用EtherType。ラボ専用LANでだけ使用する。
BLOCKED_TYPE = 0x88B5
ALLOWED_TYPE = 0x88B6
ETHERNET_HEADER_BYTES = 14
ETH_P_ALL = 0x0003
TOKEN_BYTES = 32
PROBE_COUNT = 3
RECEIVED = Path("/run/amitoki-raw-probe.jsonl")


def start_receiver():
    connection = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(ETH_P_ALL))
    connection.bind(("client0", 0))
    threading.Thread(target=receive, args=(connection,), daemon=True).start()


def receive(connection):
    with connection:
        while True:
            frame = connection.recv(65535)
            ether_type = int.from_bytes(frame[12:14], "big")
            if ether_type not in (BLOCKED_TYPE, ALLOWED_TYPE):
                continue
            token = frame[ETHERNET_HEADER_BYTES:ETHERNET_HEADER_BYTES + TOKEN_BYTES].decode("ascii")
            with RECEIVED.open("a") as stream:
                stream.write(json.dumps({"token": token, "ether_type": ether_type}) + "\n")


def send(target, token, ether_type):
    assert ether_type in (BLOCKED_TYPE, ALLOWED_TYPE)
    assert len(token) == TOKEN_BYTES
    with socket.socket(socket.AF_PACKET, socket.SOCK_RAW) as connection:
        connection.bind(("client0", 0))
        source = connection.getsockname()[4]
        frame = bytes.fromhex(target.replace(":", "")) + source + ether_type.to_bytes(2, "big") + token.encode() + bytes(18)
        for _ in range(PROBE_COUNT):
            connection.send(frame)
    return {"frames": PROBE_COUNT, "ether_type": ether_type}


def received(token):
    records = [json.loads(line) for line in RECEIVED.read_text().splitlines()] if RECEIVED.exists() else []
    return sum(record["token"] == token for record in records)
