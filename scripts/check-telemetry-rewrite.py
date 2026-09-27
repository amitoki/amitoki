"""実プロセスの加工→解析をPCAPで通し、加工後の内容と経路を照合する。"""

import hashlib
import json
from pathlib import Path
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
BINARY = ROOT / "target/release/amitoki"


def main():
    with tempfile.TemporaryDirectory(prefix="amitoki-rewrite-") as name:
        directory = Path(name)
        store = directory / "plugins"
        for plugin in ("telemetry", "telemetry-rewrite"):
            subprocess.run([str(BINARY), "plugin", "--directory", str(store), "stage", "add", str(ROOT / "dist" / plugin)], check=True, capture_output=True)
        # codecを呼ばずに既知のwire形式を作り、encode/decode双方の誤りを隠さない。
        original = bytes([3] * 12) + bytes.fromhex("88b5") + b"AMTK" + bytes([1, 0]) + struct.pack("!QhH", 42, 99, 3) + b"abc" + bytes(25)
        expected = original[:28] + struct.pack("!h", 25) + original[30:32] + bytes(28)
        pcap = directory / "input.pcap"
        pcap.write_bytes(struct.pack("<IHHIIII", 0xa1b2c3d4, 2, 4, 0, 0, 65535, 1) + struct.pack("<IIII", 0, 0, len(original), len(original)) + original)
        config = directory / "pipeline.toml"
        config.write_text('''node_id="debug"
interface="unused"
channel="debug"
[firewall]
policy="blacklist"
[[pipeline.relays]]
id="sink"
plugin="memory"
[[pipeline.stages]]
id="rewrite"
plugin="telemetry-rewrite"
[pipeline.stages.options]
temperature=25
redact_payload=true
[[pipeline.stages]]
id="parse"
plugin="telemetry"
[[pipeline.routes]]
from="capture"
to=["rewrite"]
[[pipeline.routes]]
from="rewrite.pass"
to=["parse"]
[[pipeline.routes]]
from="rewrite.drop"
to=[]
[[pipeline.routes]]
from="parse.pass"
to=["sink"]
[[pipeline.routes]]
from="parse.drop"
to=[]
[[pipeline.routes]]
from="sink.received"
to=[]
''')
        completed = subprocess.run([str(BINARY), "debug", "--directory", str(store), "replay", "--config", str(config), "--pcap", str(pcap), "--json"], check=True, capture_output=True, text=True)
        report = json.loads(completed.stdout)
        assert report["error"] is None and report["rejection"] is None, report
        assert report["terminals"] == ["sink"], report
        assert report["steps"][0]["rewrite"] == {"length": len(expected), "sha256": hashlib.sha256(expected).hexdigest()}, report
        assert report["steps"][1]["annotations"]["telemetry"]["temperature"] == 25, report
        print(json.dumps(report))


if __name__ == "__main__":
    main()
