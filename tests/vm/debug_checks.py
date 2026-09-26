"""稼働中の3台で、別プロセスのPCAP再生と種類別CLIを検証する。"""
import json

import remote
from settings import NODES

# 保存パケットだけを使い、稼働サービスの接続先・環境変数は変更しない。
DEBUG_CHECK = r'''
from pathlib import Path
import hashlib, json, struct, subprocess, tempfile
core = "/opt/amitoki-lab/amitoki"
store = "/opt/amitoki-lab/plugins"
configuration = "/opt/amitoki-lab/amitoki.toml"
def invoke(arguments):
    return subprocess.check_output([core] + arguments, text=True)
def reports(arguments):
    return [json.loads(line) for line in invoke(arguments).splitlines()]
def digest():
    with open(core, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()
before = digest()
with tempfile.TemporaryDirectory(prefix="amitoki-debug-") as directory:
    directory = Path(directory)
    capture = directory / "input.pcap"
    packets = [bytes.fromhex("020000000012020000000011") + struct.pack("!H", ether_type) + b"amitoki-debug" for ether_type in (34998, 34997)]
    with capture.open("wb") as stream:
        stream.write(struct.pack("<IHHIIII", 0xa1b2c3d4, 2, 4, 0, 0, 65535, 1))
        for packet in packets:
            stream.write(struct.pack("<IIII", 1, 0, len(packet), len(packet)) + packet)
    plugin = ["plugin", "block", "--directory", store]
    assert "packet-rules" in invoke(plugin + ["list"])
    assert json.loads(invoke(plugin + ["describe", "packet-rules"]))["block"]
    unit = reports(plugin + ["test", "/opt/amitoki-lab/packages/packet-rules", "--pcap", str(capture), "--set", "allowed_ether_types=[34998]", "--json"])
    assert [packet["terminals"] for packet in unit] == [["output:pass"], ["output:drop"]]
    replay = ["debug", "--directory", store, "replay", "--config", configuration, "--pcap", str(capture), "--json"]
    outgoing = reports(replay)
    assert outgoing[0]["terminals"] and not outgoing[1]["terminals"]
    source = outgoing[0]["terminals"][0] + ".received"
    incoming = reports(replay + ["--source", source])
    assert [packet["terminals"] for packet in incoming] == [["inject"], []]
    first, second = directory / "before.jsonl", directory / "after.jsonl"
    first.write_text("\n".join(json.dumps(packet) for packet in outgoing) + "\n")
    second.write_text(invoke(replay))
    invoke(["debug", "compare", str(first), str(second)])
assert digest() == before
print(json.dumps({"block_test": True, "capture_trace": True, "received_trace": True, "repeat_comparison": True, "core_sha256_unchanged": True}))
'''


def run():
    return {node: json.loads(remote.run(node, "sudo -u amitoki python3 -", input_text=DEBUG_CHECK).stdout)
            for node in NODES}
