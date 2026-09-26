"""専用SSH鍵によるゲスト内コマンド実行とファイル配備。"""

import shlex
import subprocess
import tarfile

from configuration import relay_configuration

from settings import DEPLOY_TIMEOUT_SECONDS, REMOTE_TIMEOUT_SECONDS, ROOT, SSH_PORTS, SSH_TIMEOUT_SECONDS, STATE


def ssh_arguments(node):
    return [
        "ssh", "-i", str(STATE / "id_ed25519"),
        "-o", "IdentitiesOnly=yes", "-o", "BatchMode=yes",
        "-o", f"ConnectTimeout={SSH_TIMEOUT_SECONDS}",
        "-o", "StrictHostKeyChecking=accept-new",
        "-o", f"UserKnownHostsFile={STATE / 'known_hosts'}",
        "-p", str(SSH_PORTS[node]), "ubuntu@127.0.0.1",
    ]


def run(node, command, *, input_text=None, check=True, timeout=REMOTE_TIMEOUT_SECONDS):
    return subprocess.run(
        ssh_arguments(node) + [command], input=input_text, text=True,
        capture_output=True, check=check, timeout=timeout,
    )


def deploy(node, password, relay, *, pipeline=False):
    from settings import DATABASE_PORT

    directory = STATE / node
    run(node, "sudo systemctl stop amitoki.service 2>/dev/null || true")
    host = "127.0.0.1" if node == "a" else "10.0.2.2"
    port = 5432 if node == "a" else DATABASE_PORT
    environment = directory / "postgres.env"
    environment.write_text(
        f'AMITOKI_POSTGRES_URL="host={host} port={port} user=amitoki '
        f'dbname=amitoki password={password} sslmode=disable"\n'
        f'AMITOKI_LAB_PASSWORD={password}\n'
    )
    environment.chmod(0o600)
    configuration = directory / "amitoki.toml"
    configuration.write_text(relay_configuration(node, relay, pipeline))
    archive = directory / "deploy.tar"
    with tarfile.open(archive, "w") as bundle:
        for filename in ("configure.sh", "network.sh", "probe.py", "raw_probe.py", "amitoki.service", "amitoki-network.service"):
            bundle.add(ROOT / "tests/vm/guest" / filename, arcname=filename)
        bundle.add(ROOT / "target/release/amitoki", arcname="amitoki")
        bundle.add(ROOT / "plugins/postgres/schema.sql", arcname="schema.sql")
        bundle.add(environment, arcname="postgres.env")
        bundle.add(configuration, arcname="amitoki.toml")
        for plugin in ("postgres", "p2p", "packet-rules"):
            for filename in ("plugin.json", f"amitoki-plugin-{plugin}"):
                bundle.add(STATE / "packages" / plugin / filename, arcname=f"packages/{plugin}/{filename}")
        bundle.add(STATE / "identities" / node / "key.der", arcname="identity/key.der")
        for peer in ("a", "b", "c"):
            bundle.add(STATE / "identities" / peer / "cert.der", arcname=f"identity/{peer}.der")
    archive.chmod(0o600)
    try:
        with archive.open("rb") as stream:
            subprocess.run(
                ssh_arguments(node) + ["sudo mkdir -p /opt/amitoki-lab && sudo tar -x -C /opt/amitoki-lab"],
                stdin=stream, check=True, timeout=REMOTE_TIMEOUT_SECONDS,
            )
        completed = run(node, f"sudo bash /opt/amitoki-lab/configure.sh {shlex.quote(node)} {shlex.quote(relay)}", timeout=DEPLOY_TIMEOUT_SECONDS)
        print(completed.stdout, end="", flush=True)
    finally:
        archive.unlink(missing_ok=True)


def collect_logs(node, destination):
    completed = run(
        node, "sudo journalctl -u amitoki --no-pager -n 200; sudo systemctl status amitoki --no-pager",
        check=False,
    )
    (destination / f"node-{node}.log").write_text(completed.stdout + completed.stderr)
