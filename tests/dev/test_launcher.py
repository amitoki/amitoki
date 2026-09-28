"""実際のAPI/Viteを起動し、認証と子プロセスの後始末を確認する。"""

import asyncio
import json
import os
from pathlib import Path
import signal
import socket
import struct
import sys
import tempfile
import unittest
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[2]
BINARY = (ROOT / os.environ.get("AMITOKI_DEV_BINARY", "target/debug/amitoki")).resolve()
# ビルド済みのサーバだけを使い、起動失敗をテスト自体のハングにしない。
START_TIMEOUT_SECONDS = 30
STOP_TIMEOUT_SECONDS = 12


def reserve_port():
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen()
    return listener


def process_children(pid):
    # uvはメインスレッド以外からも子を起動する。
    direct = set()
    for children in Path(f"/proc/{pid}/task").glob("*/children"):
        try:
            direct.update(int(value) for value in children.read_text().split())
        except FileNotFoundError:
            pass
    direct = sorted(direct)
    return direct + [child for parent in direct for child in process_children(parent)]


class LauncherTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="amitoki-dev-test-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.stderr = tempfile.TemporaryFile()
        self.addCleanup(self.stderr.close)
        self.children = []
        self.addCleanup(self.assert_children_stopped)

    def assert_children_stopped(self):
        # 終了した子は /proc から消える。残った場合は後続テストへ持ち越さない。
        remaining = []
        for pid in self.children:
            stat = Path(f"/proc/{pid}/stat")
            try:
                state = stat.read_text().split(") ", 1)[1].split()[0]
            except (FileNotFoundError, ProcessLookupError):
                continue
            if state != "Z":
                remaining.append(pid)
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        self.assertFalse(remaining, f"子プロセスが残っています: {remaining}")

    async def launch(self, *arguments):
        uv = os.environ.get("AMITOKI_DEV_UV")
        runner = [uv, "run", "--python", sys.executable] if uv else [sys.executable]
        process = await asyncio.create_subprocess_exec(
            *runner, str(ROOT / "scripts/dev.py"), "--no-build", "--binary", str(BINARY),
            "--port", "0", *map(str, arguments), cwd=self.directory,
            stdout=asyncio.subprocess.PIPE, stderr=self.stderr, start_new_session=True,
        )
        self.addAsyncCleanup(self.stop, process)
        return process

    async def stop(self, process):
        if process.returncode is None:
            process.send_signal(signal.SIGTERM)
            await asyncio.wait_for(process.wait(), STOP_TIMEOUT_SECONDS)

    async def ready_url(self, process):
        async with asyncio.timeout(START_TIMEOUT_SECONDS):
            while line := await process.stdout.readline():
                if line.startswith(b"http://"):
                    self.children.extend(process_children(process.pid))
                    return line.decode().strip()
        self.stderr.seek(0)
        self.fail(self.stderr.read().decode())

    def fetch(self, url, path, *, authenticated=True, origin=None):
        address = urlsplit(url)
        headers = {"Authorization": f"Bearer {address.fragment}"} if authenticated else {}
        if origin:
            headers["Origin"] = origin
        request = Request(f"http://{address.netloc}{path}", headers=headers)
        return urlopen(request, timeout=START_TIMEOUT_SECONDS)

    async def test_default_config_loads_pcap_and_ctrl_c_stops_both_services(self):
        # packet-rulesの設定に合うEtherTypeを持つ既知のパケット。
        packet = bytes.fromhex("02000000002002000000001088b6") + bytes(46)
        capture = self.directory / "input with spaces.pcap"
        capture.write_bytes(struct.pack("<IHHIIII", 0xa1b2c3d4, 2, 4, 0, 0, 65535, 1) + struct.pack("<IIII", 0, 0, len(packet), len(packet)) + packet)
        process = await self.launch("--pcap", capture)
        url = await self.ready_url(process)
        with self.fetch(url, "/") as response:
            self.assertIn(b"/@vite/client", response.read())
        with self.fetch(url, "/api/capture") as response:
            capture = json.load(response)
        self.assertEqual(len(capture["packets"]), 1)
        self.assertEqual(capture["packets"][0]["terminals"], ["output"])
        with self.assertRaises(HTTPError) as denied:
            self.fetch(url, "/api/topology", authenticated=False)
        self.assertEqual(denied.exception.code, 401)
        denied.exception.close()
        with self.assertRaises(HTTPError) as denied:
            self.fetch(url, "/api/topology", origin="https://attacker.example")
        self.assertEqual(denied.exception.code, 403)
        denied.exception.close()
        process.send_signal(signal.SIGINT)
        self.assertEqual(await asyncio.wait_for(process.wait(), STOP_TIMEOUT_SECONDS), 130)

    async def test_custom_relative_config_is_resolved_from_the_callers_directory(self):
        config = self.directory / "custom.toml"
        config.write_text((ROOT / "amitoki.debug.example.toml").read_text().replace('node_id = "debug"', 'node_id = "custom-dev"'))
        process = await self.launch("--config", config.name, "--directory", "./plugins")
        url = await self.ready_url(process)
        with self.fetch(url, "/api/topology") as response:
            self.assertEqual(json.load(response)["node_id"], "custom-dev")
        process.send_signal(signal.SIGTERM)
        self.assertEqual(await asyncio.wait_for(process.wait(), STOP_TIMEOUT_SECONDS), 143)

    async def test_an_occupied_frontend_port_stops_the_started_api(self):
        occupied = reserve_port()
        self.addCleanup(occupied.close)
        api_port = reserve_port()
        port = api_port.getsockname()[1]
        api_port.close()
        process = await self.launch("--port", occupied.getsockname()[1], "--api-port", port)
        self.assertEqual(await asyncio.wait_for(process.wait(), START_TIMEOUT_SECONDS), 1)
        with socket.socket() as probe:
            self.assertNotEqual(probe.connect_ex(("127.0.0.1", port)), 0)

    async def test_a_crashed_api_stops_vite_and_returns_failure(self):
        process = await self.launch()
        url = await self.ready_url(process)
        api = next(pid for pid in self.children if Path(f"/proc/{pid}/exe").resolve() == BINARY)
        os.kill(api, signal.SIGKILL)
        self.assertEqual(await asyncio.wait_for(process.wait(), STOP_TIMEOUT_SECONDS), 1)
        with socket.socket() as probe:
            self.assertNotEqual(probe.connect_ex(("127.0.0.1", urlsplit(url).port)), 0)


if __name__ == "__main__":
    unittest.main()
