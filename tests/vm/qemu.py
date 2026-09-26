"""KVMゲストの起動・状態確認・終了。PIDではなく専用QMPで操作する。"""

import json
import os
import socket
import subprocess
import time

import remote
from settings import (
    BOOT_TIMEOUT_SECONDS, CPU_COUNT, DATABASE_PORT, MEMORY_MIB,
    POLL_INTERVAL_SECONDS, PROGRESS_INTERVAL_SECONDS, SHUTDOWN_TIMEOUT_SECONDS,
    SSH_PORTS, SSH_TIMEOUT_SECONDS, STATE, P2P_PORTS, P2P_GUEST_PORT,
)


def command(node, operation):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(SSH_TIMEOUT_SECONDS)
        connection.connect(str(STATE / node / "qmp.sock"))
        stream = connection.makefile("rwb")
        json.loads(stream.readline())
        for request in ("qmp_capabilities", operation):
            stream.write(json.dumps({"execute": request}).encode() + b"\n")
            stream.flush()
            while True:
                response = json.loads(stream.readline())
                if "error" in response:
                    raise RuntimeError(response["error"])
                if "return" in response:
                    break
        return response["return"]


def is_running(node):
    try:
        command(node, "query-status")
        return True
    except (FileNotFoundError, ConnectionRefusedError):
        return False


def start(node):
    if is_running(node):
        return
    if not os.access("/dev/kvm", os.R_OK | os.W_OK):
        raise RuntimeError("/dev/kvmへの読み書き権限が必要です")
    directory = STATE / node
    (directory / "qmp.sock").unlink(missing_ok=True)
    forward = f"user,id=control,hostfwd=tcp:127.0.0.1:{SSH_PORTS[node]}-:22"
    forward += f",hostfwd=udp:127.0.0.1:{P2P_PORTS[node]}-:{P2P_GUEST_PORT}"
    if node == "a":
        forward += f",hostfwd=tcp:127.0.0.1:{DATABASE_PORT}-:5432"
    subprocess.run([
        "qemu-system-x86_64", "-name", f"amitoki-lab-{node}", "-enable-kvm", "-cpu", "host",
        "-smp", str(CPU_COUNT), "-m", str(MEMORY_MIB), "-display", "none", "-daemonize",
        "-drive", f"file={directory / 'disk.qcow2'},format=qcow2,if=virtio",
        "-drive", f"file={directory / 'seed.iso'},format=raw,media=cdrom,readonly=on",
        "-netdev", forward, "-device", "virtio-net-pci,netdev=control",
        "-serial", f"file:{directory / 'serial.log'}",
        "-qmp", f"unix:{directory / 'qmp.sock'},server=on,wait=off",
        "-pidfile", str(directory / "qemu.pid"),
    ], check=True)


def wait_ready(node):
    deadline = time.monotonic() + BOOT_TIMEOUT_SECONDS
    last_progress = 0
    while time.monotonic() < deadline:
        if not is_running(node):
            raise RuntimeError(f"VM {node}が停止しました。serial.logを確認してください")
        try:
            ready = remote.run(node, "test -f /var/lib/cloud/instance/boot-finished", check=False, timeout=10)
            if ready.returncode == 0:
                status = remote.run(node, "cloud-init status --format json", check=False)
                report = json.loads(status.stdout)
                if report.get("status") != "done":
                    raise RuntimeError(f"VM {node}のcloud-initが完了していません: {status.stdout}")
                return
        except subprocess.TimeoutExpired:
            pass
        if time.monotonic() - last_progress > PROGRESS_INTERVAL_SECONDS:
            print(f"VM {node}: OSとパッケージの準備待ち", flush=True)
            last_progress = time.monotonic()
        time.sleep(POLL_INTERVAL_SECONDS)
    raise TimeoutError(f"VM {node}の起動待ちが期限を超えました")


def stop(node):
    if not is_running(node):
        return
    command(node, "system_powerdown")
    # systemdがDBと中継を正常終了する猶予。期限後は自分のQMP接続で終了する。
    deadline = time.monotonic() + SHUTDOWN_TIMEOUT_SECONDS
    while is_running(node) and time.monotonic() < deadline:
        time.sleep(POLL_INTERVAL_SECONDS)
    if is_running(node):
        command(node, "quit")
