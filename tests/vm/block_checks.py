"""複数ブロックの起動・権限・実パケットの拒否と重複排除を確認する。"""
import json
import uuid

import remote
from settings import NODES

# ゲスト側では情報を照合し、環境変数やコマンドラインの値を出力しない。
PROCESS_CHECK = r'''
from pathlib import Path
import json, subprocess
parent = subprocess.check_output(["systemctl", "show", "amitoki", "-p", "MainPID", "--value"], text=True).strip()
children = set()
for task in Path(f"/proc/{parent}/task").iterdir():
    children.update((task / "children").read_text().split())
blocks = relays = 0
for child in children:
    process = Path(f"/proc/{child}")
    name = (process / "exe").resolve().name
    if not name.startswith("amitoki-plugin-"):
        continue
    status = dict(line.split(":", 1) for line in (process / "status").read_text().splitlines())
    assert all(int(status[key].strip(), 16) == 0 for key in ("CapEff", "CapPrm", "CapInh", "CapAmb"))
    assert status["NoNewPrivs"].strip() == "1"
    if name == "amitoki-plugin-packet-rules":
        blocks += 1
    else:
        relays += 1
assert blocks == 3
print(json.dumps({"blocks": blocks, "relays": relays, "plugin_capabilities_empty": True}))
'''


def run(relay):
    from scenarios import PROBE, database_count, wait_for
    processes = {}
    for node in NODES:
        processes[node] = json.loads(remote.run(node, "sudo python3 -", input_text=PROCESS_CHECK).stdout)
        assert processes[node]["relays"] == (2 if relay == "both" else 1)
    blocked, allowed = uuid.uuid4().hex, uuid.uuid4().hex
    remote.run("a", f"{PROBE} raw 02:00:00:00:00:12 {blocked} 34997")
    expected = json.loads(remote.run("a", f"{PROBE} raw 02:00:00:00:00:12 {allowed} 34998").stdout)["frames"]
    wait_for(lambda: int(remote.run("b", f"{PROBE} raw-received {allowed}").stdout) >= expected,
             "許可したEtherTypeが到達しません")
    if relay in ("postgres", "both"):
        wait_for(lambda: database_count("SELECT count(*) FROM stegrdb_relay.pending WHERE channel='vm-lab' AND node_id='vm-b'") == 0,
                 "受信後のACKが完了しません")
    assert int(remote.run("b", f"{PROBE} raw-received {blocked}").stdout) == 0
    assert int(remote.run("b", f"{PROBE} raw-received {allowed}").stdout) == expected
    return {"processes": processes, "blocked_frames_received": 0, "allowed_frames_received": expected}
