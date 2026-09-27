"""中継経路の障害を挟んで実通信を継続し、転送量とメモリ推移を保存する。"""

from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
import json
import time
import uuid

import remote
import scenarios
from settings import NODES, ROOT

# 1周期で全ノードを検証し、SSHと集計の時間も含む実測値を記録する。
MIN_DURATION_SECONDS = 60
PROGRESS_INTERVAL_SECONDS = 20
CLIENT_TARGETS = {"a": "192.0.2.12", "b": "192.0.2.13", "c": "192.0.2.11"}
SAMPLE_COMMAND = r"""sudo python3 - <<'PY'
import json
from pathlib import Path
import subprocess
import re
pid = int(subprocess.check_output(['systemctl','show','amitoki','--property=MainPID','--value'], text=True))
if pid <= 0:
    raise RuntimeError('amitoki service is stopped')
def process_tree(pid):
    status = Path(f'/proc/{pid}/status').read_text().splitlines()
    rss = int(next(line.split()[1] for line in status if line.startswith('VmRSS:')))
    children = {int(child) for task in Path(f'/proc/{pid}/task').iterdir() for child in (task/'children').read_text().split()}
    return rss + sum(process_tree(child) for child in children)
socket_stats = subprocess.check_output(['ss','--packet','-am'], text=True)
relay_stats = next(line for line in socket_stats.splitlines() if ':relay0' in line)
drops = int(re.search(r',d(\d+)\)', relay_stats).group(1))
print(json.dumps({'pid':pid, 'rss_with_plugins_kib':process_tree(pid), 'packet_socket_drops':drops}))
PY"""


def exercise(node):
    target = CLIENT_TARGETS[node]
    started = time.monotonic()
    try:
        tcp = json.loads(remote.run(node, f"{scenarios.PROBE} tcp {target}").stdout)
        udp = json.loads(remote.run(node, f"{scenarios.PROBE} udp {target} {uuid.uuid4().hex}").stdout)
    except Exception as error:
        raise RuntimeError(f"{node}→{target}の通信検証に失敗: {type(error).__name__}") from error
    status = json.loads(remote.run(node, SAMPLE_COMMAND).stdout)
    return {"node": node, "target": target, "cycle_seconds": time.monotonic() - started,
            "tcp": tcp, "udp_packets_verified": len(udp), **status}


def run(duration, fault):
    if duration < MIN_DURATION_SECONDS:
        raise ValueError(f"durationは{MIN_DURATION_SECONDS}秒以上にしてください")
    timestamp = datetime.now().astimezone()
    destination = ROOT / "artifacts/endurance" / timestamp.strftime("%Y-%m-%d/%H%M%S")
    destination.mkdir(parents=True)
    report = {"started_at": timestamp.isoformat(), "requested_seconds": duration, "fault": fault,
              "status": "failed", "samples": [], "events": []}
    restore_database = False
    started = None
    try:
        scenarios.prepare_receivers("both")
        baseline = {node: json.loads(remote.run(node, SAMPLE_COMMAND).stdout) for node in NODES}
        report["baseline"] = baseline
        started = time.monotonic()
        next_progress = started
        fault_applied = False
        fault_recovered = False
        with ThreadPoolExecutor(max_workers=len(NODES)) as executor:
            while time.monotonic() - started < duration:
                elapsed = time.monotonic() - started
                if fault == "postgres" and not fault_applied and elapsed >= duration / 3:
                    # finallyで必ず復旧する。既存の本体とP2Pは再起動しない。
                    restore_database = True
                    remote.run("a", "sudo systemctl stop postgresql")
                    fault_applied = True
                    report["events"].append({"elapsed_seconds": elapsed, "postgres": "stopped"})
                    print("PostgreSQL停止: P2P経由で通信を継続します", flush=True)
                if fault_applied and not fault_recovered and elapsed >= duration * 2 / 3:
                    remote.run("a", "sudo systemctl start postgresql")
                    restore_database = False
                    fault_recovered = True
                    report["events"].append({"elapsed_seconds": elapsed, "postgres": "started"})
                    print("PostgreSQL復旧: 通信を続けて確認します", flush=True)
                samples = list(executor.map(exercise, NODES))
                for sample in samples:
                    if sample["pid"] != baseline[sample["node"]]["pid"]:
                        raise RuntimeError(f'{sample["node"]}の本体が試験中に再起動しました')
                report["samples"].append({"elapsed_seconds": time.monotonic() - started,
                                          "postgres_stopped": restore_database, "nodes": samples})
                if time.monotonic() >= next_progress:
                    print(f"{time.monotonic() - started:.0f}/{duration}秒: 3経路のTCP内容・UDP応答一致", flush=True)
                    next_progress = time.monotonic() + PROGRESS_INTERVAL_SECONDS
        if fault == "postgres":
            if not fault_recovered or not any(sample["postgres_stopped"] for sample in report["samples"]):
                raise RuntimeError("障害中と復旧後の検証が完了していません")
            # PostgreSQLの占有ロック消失は恒久エラー。通信継続の検証後に明示的に再起動する。
            for node in NODES:
                remote.run(node, "sudo systemctl restart amitoki")
            scenarios.wait_for(lambda: scenarios.database_count("SELECT count(*) FROM stegrdb_relay.pending WHERE channel='vm-lab'") == 0,
                               "復旧したPostgreSQLの配送待ちが解消しません")
            scenarios.check_connectivity()
            report["recovery_required_core_restart"] = True
            report["postgres_backlog_drained"] = True
        report["status"] = "passed"
    except BaseException as error:
        report["error"] = str(error)
        raise
    finally:
        try:
            if restore_database:
                remote.run("a", "sudo systemctl start postgresql")
        finally:
            save_report(destination, report, started)


def save_report(destination, report, started):
    report["elapsed_seconds"] = time.monotonic() - started if started else 0
    flat = [node for sample in report["samples"] for node in sample["nodes"]]
    report["tcp_bytes_verified"] = sum(node["tcp"]["bytes"] for node in flat)
    report["udp_packets_verified"] = sum(node["udp_packets_verified"] for node in flat)
    report["peak_rss_with_plugins_kib"] = {node: max((entry["rss_with_plugins_kib"] for entry in flat if entry["node"] == node), default=0) for node in NODES}
    (destination / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"耐久試験: {report['status']} / 記録: {destination}", flush=True)
