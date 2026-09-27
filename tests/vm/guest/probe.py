"""テストLAN上のTCP転送・UDP内容照合。制御用NICにはbindしない。"""

import hashlib
import json
from pathlib import Path
import socket
import socketserver
import raw_probe
import struct
import sys
import threading
import time

# MTU1500でフラグメントしない最大UDPデータ長。短い機能試験として64件を照合する。
UDP_BYTES = 1472
UDP_COUNT = 64
TCP_BYTES = 2 * 1024 * 1024
TCP_PORT = 19001
UDP_PORT = 19000
TIMEOUT_SECONDS = 30
RECEIVED = Path("/run/amitoki-probe.jsonl")
READY = Path("/run/amitoki-probe.ready")


class TcpReceiver(socketserver.BaseRequestHandler):
    def handle(self):
        digest = hashlib.sha256()
        count = 0
        self.request.settimeout(TIMEOUT_SECONDS)
        while chunk := self.request.recv(65536):
            digest.update(chunk)
            count += len(chunk)
        self.request.sendall(json.dumps({"bytes": count, "sha256": digest.hexdigest()}).encode())


class UdpReceiver(socketserver.BaseRequestHandler):
    def handle(self):
        payload, connection = self.request
        if len(payload) != UDP_BYTES:
            return
        record = {
            "token": payload[:32].decode("ascii"), "sequence": struct.unpack("!I", payload[32:36])[0],
            "sha256": hashlib.sha256(payload).hexdigest(), "bytes": len(payload),
        }
        with RECEIVED.open("a") as stream:
            stream.write(json.dumps(record) + "\n")
        connection.sendto(payload, self.client_address)


def serve():
    address_number = {"a": 11, "b": 12, "c": 13}[Path("/opt/amitoki-lab/node").read_text().strip()]
    address = f"192.0.2.{address_number}"
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.ThreadingTCPServer((address, TCP_PORT), TcpReceiver) as tcp:
        with socketserver.UDPServer((address, UDP_PORT), UdpReceiver) as udp:
            threading.Thread(target=tcp.serve_forever, daemon=True).start()
            raw_probe.start_receiver()
            READY.touch()
            udp.serve_forever()


def tcp_transfer(address):
    payload = bytes(range(256)) * (TCP_BYTES // 256)
    started = time.monotonic()
    with socket.create_connection((address, TCP_PORT), timeout=TIMEOUT_SECONDS) as connection:
        connection.sendall(payload)
        connection.shutdown(socket.SHUT_WR)
        response = b""
        while chunk := connection.recv(4096):
            response += chunk
    report = json.loads(response)
    assert report == {"bytes": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}, report
    report["elapsed_seconds"] = time.monotonic() - started
    report["payload_mbps"] = len(payload) * 8 / report["elapsed_seconds"] / 1_000_000
    return report


def udp_transfer(address, token, *, await_echo):
    expected = {}
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
        connection.settimeout(TIMEOUT_SECONDS)
        for sequence in range(UDP_COUNT):
            payload = token.encode() + struct.pack("!I", sequence) + bytes([sequence]) * (UDP_BYTES - 36)
            connection.sendto(payload, (address, UDP_PORT))
            if await_echo:
                reply, _ = connection.recvfrom(65535)
                assert reply == payload, sequence
            expected[str(sequence)] = hashlib.sha256(payload).hexdigest()
    return expected


def received(token):
    records = [json.loads(line) for line in RECEIVED.read_text().splitlines()] if RECEIVED.exists() else []
    return {str(record["sequence"]): record["sha256"] for record in records if record["token"] == token}


if __name__ == "__main__":
    action = sys.argv[1]
    if action == "serve":
        serve()
    elif action == "tcp":
        print(json.dumps(tcp_transfer(sys.argv[2])))
    elif action in ("udp", "send-udp"):
        print(json.dumps(udp_transfer(sys.argv[2], sys.argv[3], await_echo=action == "udp")))
    elif action == "raw":
        print(json.dumps(raw_probe.send(sys.argv[2], sys.argv[3], int(sys.argv[4]))))
    elif action == "raw-received":
        print(json.dumps(raw_probe.received(sys.argv[2])))
    elif action == "received":
        print(json.dumps(received(sys.argv[2])))
    else:
        raise SystemExit(f"不明な操作: {action}")
