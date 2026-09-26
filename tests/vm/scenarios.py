"""3台のゲスト間で、中継方式ごとの実通信と停止後の再配送を確認する。"""

from datetime import datetime
import hashlib
import json
import time
import uuid

import block_checks
import qemu
import remote
from settings import NODES, POLL_INTERVAL_SECONDS, PROBE_TIMEOUT_SECONDS, ROOT

PROBE = "sudo ip netns exec client python3 /opt/amitoki-lab/probe.py"
# MTU上限のICMPを3回送り、一部だけ届く状態も失敗にする。
ICMP_PACKETS = 3
ICMP_BYTES = 1472


def wait_for(check, description):
    deadline = time.monotonic() + PROBE_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(POLL_INTERVAL_SECONDS)
    raise TimeoutError(description)


def database_count(sql):
    # SQLはこのファイルの固定文字列だけを渡す。
    completed = remote.run("a", f'sudo -u postgres psql -d amitoki -Atc "{sql}"')
    return int(completed.stdout.strip())


def prepare_receivers(relay):
    for node in NODES:
        if not qemu.is_running(node):
            raise RuntimeError("先にscripts/vm-lab upを実行してください")
        wait_for(lambda: remote.run(node, "systemctl is-active --quiet amitoki", check=False).returncode == 0,
                 f"VM {node}の中継が起動していません")
        remote.run(node, "sudo systemctl stop amitoki-probe 2>/dev/null || true; sudo rm -f /run/amitoki-probe.ready")
        remote.run(node, "sudo systemd-run --unit=amitoki-probe --collect "
                        "--property=NetworkNamespacePath=/run/netns/client "
                        "/usr/bin/python3 /opt/amitoki-lab/probe.py serve")
        wait_for(lambda: remote.run(node, "test -f /run/amitoki-probe.ready", check=False).returncode == 0,
                 f"VM {node}の受信サーバが起動していません")
    if relay in ("postgres", "both"):
        wait_for(lambda: database_count("SELECT count(*) FROM stegrdb_relay.nodes WHERE channel='vm-lab'") == 3,
                 "3ノードの登録が揃いません")


def check_connectivity():
    reports = []
    for source, target in (("a", "192.0.2.12"), ("b", "192.0.2.13"), ("c", "192.0.2.11")):
        ping = remote.run(source, f"sudo ip netns exec client env LC_ALL=C ping "
                                 f"-c {ICMP_PACKETS} -W 5 -M do -s {ICMP_BYTES} {target}")
        assert f"{ICMP_PACKETS} packets transmitted, {ICMP_PACKETS} received" in ping.stdout, ping.stdout
        print(ping.stdout, end="", flush=True)
        tcp = json.loads(remote.run(source, f"{PROBE} tcp {target}").stdout)
        udp = json.loads(remote.run(source, f"{PROBE} udp {target} {uuid.uuid4().hex}").stdout)
        reports.append({"source": source, "target": target, "icmp_packets": ICMP_PACKETS,
                        "icmp_payload_bytes": ICMP_BYTES, "tcp": tcp, "udp_packets": len(udp)})
    return reports


def check_offline_delivery(relay):
    token = uuid.uuid4().hex
    # 停止試験中のARP再解決に結果を左右させず、UDPの未処理キューを直接検証する。
    remote.run("a", "sudo ip netns exec client ip neigh replace 192.0.2.13 "
                    "lladdr 02:00:00:00:00:13 nud permanent dev client0")
    remote.run("c", "sudo systemctl stop amitoki")
    try:
        expected = json.loads(remote.run("a", f"{PROBE} send-udp 192.0.2.13 {token}").stdout)
        if relay == "postgres":
            wait_for(lambda: database_count("SELECT count(*) FROM stegrdb_relay.pending "
                                            "WHERE channel='vm-lab' AND node_id='vm-c'") >= len(expected),
                     "停止ノード向けのフレームがDBに揃いません")
        before = json.loads(remote.run("c", f"{PROBE} received {token}").stdout)
        assert not before, "中継停止中に別経路で届いています"
    finally:
        remote.run("c", "sudo systemctl start amitoki")
    wait_for(lambda: json.loads(remote.run("c", f"{PROBE} received {token}").stdout) == expected,
             "再起動後のUDPペイロードが送信内容と一致しません")
    if relay == "postgres":
        wait_for(lambda: database_count("SELECT count(*) FROM stegrdb_relay.pending "
                                        "WHERE channel='vm-lab' AND node_id='vm-c'") == 0,
                 "配送後にACKされていないフレームがあります")
    return {"recovered_udp_packets": len(expected), "payload_sha256_verified": True}


def check_plugin_management(relay, pipeline):
    command = "sudo -u amitoki /opt/amitoki-lab/amitoki plugin --directory /opt/amitoki-lab/plugins"
    before = remote.run("a", "sha256sum /opt/amitoki-lab/amitoki").stdout.split()[0]
    active = ["postgres", "p2p"] if relay == "both" else [relay]
    if pipeline:
        active.append("packet-rules")
    for plugin in active:
        assert remote.run("a", f"{command} remove {plugin}", check=False).returncode != 0
        assert remote.run("a", f"{command} update {plugin} --path /opt/amitoki-lab/packages/{plugin}", check=False).returncode != 0
    inactive = next((plugin for plugin in ("postgres", "p2p", "packet-rules") if plugin not in active), None)
    if inactive is None:
        # 両中継が動いている試験では、1台を停止してブロックだけ追加し直す。
        remote.run("a", "sudo systemctl stop amitoki")
        inactive = "packet-rules"
    try:
        remote.run("a", f"{command} remove {inactive}")
        remote.run("a", f"{command} add --path /opt/amitoki-lab/packages/{inactive}")
        if inactive == "postgres":
            remote.run("a", f"{command} configure postgres --set max_connections=4")
            remote.run("a", f"{command} validate postgres")
            assert remote.run("a", f"{command} configure postgres --set max_connections=0", check=False).returncode != 0
    finally:
        remote.run("a", "sudo systemctl start amitoki")
    after = remote.run("a", "sha256sum /opt/amitoki-lab/amitoki").stdout.split()[0]
    assert before == after, "プラグインの追加削除で本体が変更されています"
    with (ROOT / "target/release/amitoki").open("rb") as stream:
        expected = hashlib.file_digest(stream, "sha256").hexdigest()
    for node in NODES:
        assert remote.run(node, "sha256sum /opt/amitoki-lab/amitoki").stdout.split()[0] == expected
        assert remote.run(node, "command -v cargo", check=False).returncode != 0
    return {"active_remove_rejected": True, "active_update_rejected": True,
            "inactive_remove_and_add": True, "core_sha256_unchanged": before, "cargo_absent_on_guests": True}


def run_tests(relay, pipeline=False):
    timestamp = datetime.now().astimezone()
    destination = ROOT / "artifacts/vm" / timestamp.strftime("%Y-%m-%d/%H%M%S")
    destination.mkdir(parents=True)
    report = {"started_at": timestamp.isoformat(), "status": "failed", "relay": relay, "pipeline": pipeline}
    try:
        if relay == "p2p":
            remote.run("a", "sudo systemctl stop postgresql")
            report["postgres_stopped"] = True
        prepare_receivers(relay)
        report["connectivity"] = check_connectivity()
        if pipeline:
            report["blocks"] = block_checks.run(relay)
        report["offline_delivery"] = check_offline_delivery(relay)
        report["plugin_management"] = check_plugin_management(relay, pipeline)
        report["status"] = "passed"
        print("3台のVM: ICMP・TCP・UDP・停止後の再配送が成功しました", flush=True)
    finally:
        (destination / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        for node in NODES:
            try:
                remote.collect_logs(node, destination)
            except Exception as error:
                print(f"VM {node}のログ取得に失敗: {type(error).__name__}", flush=True)
        print(f"試験記録: {destination}", flush=True)
