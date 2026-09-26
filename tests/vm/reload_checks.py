"""実通信中のStage追加・更新・拒否・SIGHUPを、PIDとパケット到達で検証する。"""
from datetime import datetime
import json
import shlex
import subprocess
import time
import tomllib

import remote
from settings import ROOT

CONFIG = "/opt/amitoki-lab/amitoki.toml"
COMMAND = "sudo -u amitoki /opt/amitoki-lab/amitoki"
MANAGER = f"{COMMAND} plugin --directory /opt/amitoki-lab/plugins"
RELOAD = f"{COMMAND} reload --config {CONFIG}"
# SSHと子プロセスの終了を待つ期限。固定sleepではなく状態を確認する。
DRAIN_TIMEOUT_SECONDS = 30
POLL_SECONDS = 0.1
PING_COUNT = 100

SNAPSHOT = r'''
from pathlib import Path
import json, subprocess
parent = subprocess.check_output(["systemctl", "show", "amitoki", "-p", "MainPID", "--value"], text=True).strip()
children = set()
for task in Path(f"/proc/{parent}/task").iterdir():
    try: children.update((task / "children").read_text().split())
    except FileNotFoundError: pass
stages, relays = [], []
for child in children:
    try: name = (Path(f"/proc/{child}") / "exe").resolve(strict=True).name
    except FileNotFoundError: continue
    if name in ("amitoki-plugin-postgres", "amitoki-plugin-p2p"):
        relays.append(int(child))
    elif name.startswith("amitoki-plugin-"):
        stages.append(int(child))
print(json.dumps({"main":int(parent),"stages":sorted(stages),"relays":sorted(relays)}))
'''


def snapshot(node):
    return json.loads(remote.run(node, "sudo python3 -", input_text=SNAPSHOT).stdout)


def replace_configuration(node, contents):
    # 入力はstdinで渡し、シェルの文字列展開や秘密情報の出力を避ける。
    script = (
        "import os,sys,tempfile; from pathlib import Path; "
        f"path=Path({CONFIG!r}); previous=path.stat(); "
        "fd,temporary=tempfile.mkstemp(dir=path.parent); "
        "stream=os.fdopen(fd,'w'); stream.write(sys.stdin.read()); stream.close(); "
        "os.chmod(temporary,previous.st_mode); os.chown(temporary,previous.st_uid,previous.st_gid); "
        "os.replace(temporary,path)"
    )
    remote.run(node, "sudo python3 -c " + shlex.quote(script), input_text=contents)


def wait_for_stages(node, previous, count):
    deadline = time.monotonic() + DRAIN_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        current = snapshot(node)
        assert current["main"] == previous["main"], "reloadで本体PIDが変わりました"
        assert current["relays"] == previous["relays"], "reloadでRelay接続が再起動しました"
        if len(current["stages"]) == count and not set(current["stages"]) & set(previous["stages"]):
            return current
        time.sleep(POLL_SECONDS)
    raise TimeoutError("旧世代のStageが終了しません")


def check_node(node, destination):
    original = remote.run(node, f"sudo cat {CONFIG}").stdout
    # systemdのactiveはプラグイン初期化完了より早い。制御応答を受けてから基準を取る。
    remote.run(node, RELOAD)
    expected_relays = len(tomllib.loads(original)["pipeline"]["relays"])
    deadline = time.monotonic() + DRAIN_TIMEOUT_SECONDS
    while True:
        before = snapshot(node)
        if len(before["stages"]) == 3 and len(before["relays"]) == expected_relays:
            break
        if time.monotonic() >= deadline:
            raise TimeoutError("基準となるPipelineが起動・終了待ちを完了しません")
        time.sleep(POLL_SECONDS)
    (destination / f"before-{node}.json").write_text(json.dumps(before) + "\n")
    binary_hash = remote.run(node, "sha256sum /opt/amitoki-lab/amitoki").stdout.split()[0]
    target = {"a": "192.0.2.12", "b": "192.0.2.13", "c": "192.0.2.11"}[node]
    ping_command = f"sudo ip netns exec client env LC_ALL=C ping -c {PING_COUNT} -i 0.1 -W 5 {target}"
    with (destination / f"ping-{node}.log").open("w") as ping_log:
        ping = subprocess.Popen(remote.ssh_arguments(node) + [ping_command], stdout=ping_log, stderr=subprocess.STDOUT)
        try:
            if remote.run(node, f"{MANAGER} stage describe telemetry", check=False).returncode == 0:
                remote.run(node, f"{MANAGER} stage update telemetry --path /opt/amitoki-lab/packages/telemetry")
            else:
                remote.run(node, f"{MANAGER} stage add /opt/amitoki-lab/packages/telemetry")
            capture_route = 'to = ["outbound", "audit"]'
            assert original.count(capture_route) == 1
            added = original.replace(capture_route, 'to = ["outbound", "audit", "telemetry-observe"]')
            added += '\n[[pipeline.blocks]]\nid="telemetry-observe"\nplugin="telemetry"\n'
            added += '[[pipeline.routes]]\nfrom="telemetry-observe.pass"\nto=[]\n'
            added += '[[pipeline.routes]]\nfrom="telemetry-observe.drop"\nto=[]\n'
            replace_configuration(node, added)
            remote.run(node, RELOAD)
            active = wait_for_stages(node, before, 4)
            remote.run(node, f"{MANAGER} stage update packet-rules --path /opt/amitoki-lab/packages/packet-rules")
            remote.run(node, RELOAD)
            active = wait_for_stages(node, active, 4)
            # 入口からNICへの直接注入を拒否して旧世代を維持する。
            invalid = added.replace('to = ["outbound", "audit", "telemetry-observe"]', 'to = ["inject"]')
            replace_configuration(node, invalid)
            rejected = remote.run(node, RELOAD, check=False)
            assert rejected.returncode != 0
            assert snapshot(node) == active
            replace_configuration(node, original)
            remote.run(node, "sudo systemctl reload amitoki")
            after = wait_for_stages(node, active, 3)
            assert ping.wait(timeout=DRAIN_TIMEOUT_SECONDS) == 0
        finally:
            replace_configuration(node, original)
            remote.run(node, RELOAD, check=False)
            if ping.poll() is None:
                ping.terminate()
                ping.wait(timeout=DRAIN_TIMEOUT_SECONDS)
    text = (destination / f"ping-{node}.log").read_text()
    assert f"{PING_COUNT} packets transmitted, {PING_COUNT} received" in text, text
    assert binary_hash == remote.run(node, "sha256sum /opt/amitoki-lab/amitoki").stdout.split()[0]
    bench = remote.run(node, f"{MANAGER} stage bench telemetry --packet telemetry-v1 --count 1000 --json")
    benchmark = json.loads(bench.stdout)
    assert benchmark["packets"] == 1000 and benchmark["errors"] == 0
    assert remote.run(node, "command -v cargo", check=False).returncode != 0
    return {"core_pid":before["main"],"relay_pids":before["relays"],"core_sha256":binary_hash,
            "stage_add_update_reload":True,"invalid_reload_kept_old_generation":True,"sighup":True,
            "ping_packets":PING_COUNT,"stage_count_after_restore":len(after["stages"]),"benchmark":benchmark}


def run(nodes):
    timestamp = datetime.now().astimezone()
    destination = ROOT / "artifacts/vm-reload" / timestamp.strftime("%Y-%m-%d/%H%M%S")
    destination.mkdir(parents=True)
    report = {"started_at":timestamp.isoformat(),"status":"failed","nodes":{}}
    try:
        for node in nodes:
            report["nodes"][node] = check_node(node, destination)
            print(f"VM {node}: Stage追加・更新・reload拒否・SIGHUP・通信継続を確認", flush=True)
        report["status"] = "passed"
    finally:
        (destination / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        print(f"試験記録: {destination}", flush=True)
