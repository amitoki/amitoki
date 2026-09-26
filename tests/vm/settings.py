"""ローカルVMラボの配置と、外部へ公開しない接続先。"""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STATE = ROOT / ".vm-lab"
CACHE = Path.home() / ".cache/amitoki-vm"
NODES = ("a", "b", "c")
SSH_PORTS = {"a": 22221, "b": 22222, "c": 22223}
DATABASE_PORT = 25432
P2P_PORTS = {"a": 27441, "b": 27442, "c": 27443}
P2P_GUEST_PORT = 7443
IMAGE_NAME = "ubuntu-24.04-server-cloudimg-amd64.img"
IMAGE_URL = "https://cloud-images.ubuntu.com/releases/noble/release-20260801"
# 3台で合計6GiB。DBとOSの初期化を並行しても余裕を持たせる。
MEMORY_MIB = 2048
CPU_COUNT = 2
DISK_SIZE = "12G"
# 初回のapt処理も含めて待つ。状態を確認しながら期限を判定する。
BOOT_TIMEOUT_SECONDS = 600
POLL_INTERVAL_SECONDS = 1
SSH_TIMEOUT_SECONDS = 5
PROBE_TIMEOUT_SECONDS = 30
# 制御操作にも期限を設け、停止中のゲストを待ち続けない。
REMOTE_TIMEOUT_SECONDS = 60
DEPLOY_TIMEOUT_SECONDS = 120
SHUTDOWN_TIMEOUT_SECONDS = 45
PROGRESS_INTERVAL_SECONDS = 20
