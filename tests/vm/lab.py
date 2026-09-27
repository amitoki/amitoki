"""ローカルの3台のKVMを再現可能なテスト環境として管理するCLI。"""

import argparse
import fcntl
import os
import shutil
import subprocess
import sys

import images
import qemu
import remote
import scenarios
from settings import NODES, ROOT, SSH_PORTS, STATE


def up(relay, pipeline, nodes=NODES):
    subprocess.run(["bash", "scripts/build-web.sh"], cwd=ROOT, check=True)
    subprocess.run(["cargo", "build", "--release", "--bin", "amitoki", "--locked"], cwd=ROOT, check=True)
    for plugin in ("postgres", "p2p"):
        subprocess.run(["cargo", "build", "--release", "--locked", "--manifest-path", f"plugins/{plugin}/Cargo.toml"], cwd=ROOT, check=True)
        subprocess.run([sys.executable, "scripts/package-plugin.py", f"plugins/{plugin}/target/release/amitoki-plugin-{plugin}", str(STATE / "packages" / plugin)], cwd=ROOT, check=True)
    subprocess.run(["cargo", "build", "--release", "--locked", "-p", "amitoki-block-packet-rules"], cwd=ROOT, check=True)
    subprocess.run([sys.executable, "scripts/package-plugin.py", "target/release/amitoki-plugin-packet-rules", str(STATE / "packages" / "packet-rules")], cwd=ROOT, check=True)
    subprocess.run(["bash", "scripts/build-telemetry.sh"], cwd=ROOT, check=True)
    identities = STATE / "identities"
    identities.mkdir(exist_ok=True)
    for node in NODES:
        if not (identities / node).exists():
            subprocess.run([str(ROOT / "plugins/p2p/target/release/amitoki-plugin-p2p"), "identity", "--output", str(identities / node)], check=True)
    password = images.prepare_credentials()
    image = images.prepare_image()
    # まずDBを持つ1台を起動し、その準備が済んでから3台へ広げる。
    for node in NODES:
        if node not in nodes:
            continue
        images.seed_guest(node, image)
        qemu.start(node)
        qemu.wait_ready(node)
        remote.deploy(node, password, relay, pipeline=pipeline)
    (STATE / "relay-mode").write_text(relay)
    (STATE / "pipeline-mode").write_text("yes" if pipeline else "no")
    print("VMラボを起動しました。scripts/vm-lab testで通信を検証できます。", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("up", "test", "reload-test", "endurance", "status", "ssh", "down", "destroy"))
    parser.add_argument("--duration", type=int, default=300, help="enduranceの実行秒数（60以上）")
    parser.add_argument("--fault", choices=("none", "postgres"), default="none", help="enduranceで挟む障害")
    parser.add_argument("node", nargs="?", choices=NODES)
    parser.add_argument("--relay", choices=("postgres", "p2p", "both"))
    parser.add_argument("--pipeline", action=argparse.BooleanOptionalAction, default=None, help="解析・フィルタを含むブロック構成で起動")
    options = parser.parse_args()
    relay = options.relay or ((STATE / "relay-mode").read_text().strip() if (STATE / "relay-mode").exists() else "postgres")
    pipeline = options.pipeline if options.pipeline is not None else ((STATE / "pipeline-mode").read_text().strip() == "yes" if (STATE / "pipeline-mode").exists() else False)
    if relay == "both" and options.pipeline is False:
        parser.error("bothはブロック構成専用です。単一中継には--relay postgresまたはp2pを指定してください")
    pipeline = pipeline or relay == "both"
    os.umask(0o077)
    STATE.mkdir(mode=0o700, parents=True, exist_ok=True)
    # up/down/testの同時実行でディスクやサービスを入れ替えない。
    with (STATE / "lab.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if options.action == "up":
            up(relay, pipeline, (options.node,) if options.node else NODES)
        elif options.action == "test":
            scenarios.run_tests(relay, pipeline)
        elif options.action == "reload-test":
            import reload_checks
            reload_checks.run((options.node,) if options.node else NODES)
        elif options.action == "endurance":
            if relay != "both" or options.node:
                parser.error("enduranceは3台の--relay both構成で実行してください")
            import endurance
            endurance.run(options.duration, options.fault)
        elif options.action == "status":
            for node in NODES:
                print(f"{node}: {'running' if qemu.is_running(node) else 'stopped'} (SSH 127.0.0.1:{SSH_PORTS[node]})")
        elif options.action == "ssh":
            if options.node is None:
                parser.error("sshにはa/b/cを指定してください")
            # 対話SSHがラボの管理ロックを保持しないよう、exec前に解放する。
            fcntl.flock(lock, fcntl.LOCK_UN)
            os.execvp("ssh", remote.ssh_arguments(options.node))
        elif options.action in ("down", "destroy"):
            for node in reversed(NODES):
                qemu.stop(node)
            if options.action == "destroy":
                # このリポジトリ内の専用ディスクだけを削除する。キャッシュと試験記録は残す。
                for node in NODES:
                    directory = STATE / node
                    if directory.exists():
                        shutil.rmtree(directory)
                for name in ("identities", "packages"):
                    if (STATE / name).exists():
                        shutil.rmtree(STATE / name)
                for filename in ("id_ed25519", "id_ed25519.pub", "known_hosts", "postgres-password", "relay-mode", "pipeline-mode"):
                    (STATE / filename).unlink(missing_ok=True)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(error.stdout or "", file=sys.stderr)
        print(error.stderr or "", file=sys.stderr)
        raise SystemExit(f"コマンドが失敗しました（終了コード{error.returncode}）")
