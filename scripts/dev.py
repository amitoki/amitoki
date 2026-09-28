# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""uv run scripts/dev.pyでWebのビルドと開発サーバを起動する。"""

import argparse
import asyncio
import os
from pathlib import Path
import shutil
import signal
import sys
import tempfile
from urllib.parse import urlsplit

from dev_processes import managed_processes, read_ready, supervise

ROOT = Path(__file__).resolve().parent.parent
WEB = ROOT / "web"
# PCAPの初期解析にはAPI側で最大120秒かかるため、その後の起動も待てる上限。
API_START_TIMEOUT_SECONDS = 150
# Viteの初期化が止まった場合もAPIを終了できるよう、待ち時間を制限する。
VITE_START_TIMEOUT_SECONDS = 30
# Vite単体起動と同じポートを使う。
DEFAULT_WEB_PORT = 5173


def port_number(value):
    port = int(value)
    if not 0 <= port <= 65535:
        raise argparse.ArgumentTypeError("ポートは0〜65535で指定してください")
    return port


def local_path(value):
    return Path(value).expanduser().resolve()


def parse_arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=local_path, help="設定ファイル（省略時はデバッグ用サンプル）")
    parser.add_argument("--pcap", type=local_path, help="起動時に読み込むPCAP")
    parser.add_argument("--directory", type=local_path, help="プラグインの保存先")
    parser.add_argument("--port", type=port_number, default=DEFAULT_WEB_PORT, help="Viteのポート（0で自動割り当て）")
    parser.add_argument("--api-port", type=port_number, default=0, help="APIのポート（既定は自動割り当て）")
    parser.add_argument("--no-build", action="store_true", help="既存のビルドとnpm依存を使う")
    parser.add_argument("--binary", type=local_path, help="既存のamitoki実行ファイルを使う")
    return parser.parse_args()


async def prepare(processes, arguments):
    if arguments.directory is not None and arguments.config is None:
        raise RuntimeError("--directoryは--configと一緒に指定してください")
    needs_cargo = not arguments.no_build and (arguments.binary is None or arguments.config is None)
    for tool in ("node", "npm", *(("cargo",) if needs_cargo else ())):
        if shutil.which(tool) is None:
            raise RuntimeError(f"{tool}が見つかりません。docs/web-ui.mdの開発環境を確認してください")
    for path in (arguments.config, arguments.pcap, arguments.binary):
        if path is not None and not path.is_file():
            raise RuntimeError(f"ファイルがありません: {path}")
    if arguments.no_build:
        return
    await processes.run(["bash", ROOT / "scripts/build-web.sh"], cwd=ROOT)
    if arguments.binary is None:
        await processes.run(["cargo", "build", "--bin", "amitoki", "--locked"], cwd=ROOT)
    if arguments.config is None:
        await processes.run(["bash", ROOT / "scripts/build-packet-rules.sh"], cwd=ROOT)


async def serve(processes, arguments, temporary):
    binary = arguments.binary or ROOT / "target/debug/amitoki"
    if not binary.is_file() or not (WEB / "node_modules/vite/bin/vite.js").is_file():
        raise RuntimeError("ビルドがありません。--no-buildを外して起動してください")
    command = [binary, "web", "--listen", f"127.0.0.1:{arguments.api_port}"]
    config = arguments.config or ROOT / "amitoki.debug.example.toml"
    directory = arguments.directory
    if arguments.config is None:
        # サンプルを利用者のプラグイン保存先へ登録しない。
        directory = Path(temporary) / "plugins"
        await processes.run([binary, "plugin", "--directory", directory, "stage", "add", ROOT / "dist/packet-rules"], cwd=ROOT)
    command.extend(["--config", config])
    if directory is not None:
        command.extend(["--directory", directory])
    if arguments.pcap is not None:
        command.extend(["--pcap", arguments.pcap])
    api = await processes.start(command, cwd=ROOT, capture=True)
    api_url = urlsplit("http://" + await read_ready(api, prefix="http://", timeout=API_START_TIMEOUT_SECONDS))
    if api_url.hostname != "127.0.0.1" or not api_url.fragment:
        raise RuntimeError("APIの起動URLが不正です")
    vite = await processes.start(
        ["node", WEB / "dev-server.mjs", str(arguments.port)], cwd=WEB,
        env={**os.environ, "AMITOKI_WEB_BACKEND": f"http://{api_url.netloc}"}, capture=True,
    )
    frontend = await read_ready(vite, prefix="AMITOKI_VITE_READY=", timeout=VITE_START_TIMEOUT_SECONDS)
    print(f"\n{frontend}#{api_url.fragment}\nCtrl+Cで停止\n", flush=True)
    await supervise({"API": api, "Vite": vite})


async def run(arguments):
    current = asyncio.current_task()
    loop = asyncio.get_running_loop()
    for number in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(number, current.cancel, number)
    with tempfile.TemporaryDirectory(prefix="amitoki-dev-") as temporary:
        async with managed_processes() as processes:
            await prepare(processes, arguments)
            await serve(processes, arguments, temporary)


def main():
    try:
        asyncio.run(run(parse_arguments()))
    except asyncio.CancelledError as error:
        return 128 + int(error.args[0]) if error.args else 130
    except (OSError, RuntimeError, TimeoutError) as error:
        print(f"起動できません: {error or '開発サーバの起動がタイムアウトしました'}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
