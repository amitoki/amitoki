#!/usr/bin/env bash
set -euo pipefail
cd /opt/amitoki-lab
node=$1
relay=${2:-postgres}
case "$node" in a|b|c) ;; *) exit 1 ;; esac
systemctl stop amitoki.service 2>/dev/null || true
if ! id amitoki >/dev/null 2>&1; then useradd --system --shell /usr/sbin/nologin amitoki; fi
chmod 755 amitoki
chmod 644 amitoki.toml
chmod -R a+rX packages
chmod 600 postgres.env
printf '%s\n' "$node" > node
install -d -o amitoki -g amitoki /opt/amitoki-lab/plugins
chown -R amitoki:amitoki identity
chmod 700 identity
chmod 600 identity/key.der
for plugin in postgres p2p; do
  if [[ -d plugins/$plugin ]]; then
    runuser -u amitoki -- ./amitoki plugin --directory /opt/amitoki-lab/plugins update "$plugin" --path "packages/$plugin"
  else
    runuser -u amitoki -- ./amitoki plugin --directory /opt/amitoki-lab/plugins add --path "packages/$plugin"
  fi
done

if [[ $node == a && $relay == postgres ]]; then
  pg_conftool 16 main set listen_addresses '*'
  # QEMUのhost forwardingはゲストから10.0.2.2に見える。専用DB・専用ロールだけ許可する。
  rule='host amitoki amitoki 10.0.2.2/32 scram-sha-256'
  if ! grep -qF "$rule" /etc/postgresql/16/main/pg_hba.conf; then
    printf '%s\n' "$rule" >> /etc/postgresql/16/main/pg_hba.conf
  fi
  systemctl restart postgresql
  set -a
  source postgres.env
  set +a
  python3 - <<'PY'
import os
import re
import subprocess

password = os.environ["AMITOKI_LAB_PASSWORD"]
assert re.fullmatch(r"[a-f0-9]{48}", password)
prefix = ["runuser", "-u", "postgres", "--", "psql", "-v", "ON_ERROR_STOP=1"]
sql = "DO $$ BEGIN CREATE ROLE amitoki LOGIN; EXCEPTION WHEN duplicate_object THEN NULL; END $$;\n"
sql += f"ALTER ROLE amitoki PASSWORD '{password}';\n"
subprocess.run(prefix, input=sql, text=True, check=True, stdout=subprocess.DEVNULL)
exists = subprocess.check_output(prefix + ["-Atc", "SELECT 1 FROM pg_database WHERE datname='amitoki'"], text=True)
if not exists.strip():
    subprocess.run(["runuser", "-u", "postgres", "--", "createdb", "-O", "amitoki", "amitoki"], check=True)
PY
  # オブジェクトの所有者も接続ロールに揃える。
  PGPASSWORD="$AMITOKI_LAB_PASSWORD" psql -h 127.0.0.1 -U amitoki -d amitoki -v ON_ERROR_STOP=1 -f schema.sql
fi
install -m 644 amitoki.service amitoki-network.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now amitoki-network.service amitoki.service
printf 'VM %s: 中継を配備しました\n' "$node"
