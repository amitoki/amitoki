#!/usr/bin/env python3
"""配布した実行ファイルでプラグイン管理・PCAPテスト・watch・再生を試す。"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

from metadata import ROOT, output, version


def smoke(binary, plugin):
    if output(str(binary), "--version") != f"amitoki {version()}":
        raise ValueError("配布した実行ファイルの版が一致しません")
    with tempfile.TemporaryDirectory(prefix="amitoki-smoke-") as temporary:
        directory = Path(temporary)
        capture = directory / "input.pcap"
        store = directory / "plugins"
        subprocess.run(["python3", str(ROOT / "scripts/create-debug-capture.py"), str(capture)], check=True)
        def run(*arguments):
            return output(str(binary), *map(str, arguments))
        run("plugin", "block", "add", plugin, "--directory", store)
        run("plugin", "block", "configure", "packet-rules", "--set", "label=release-smoke", "--directory", store)
        run("plugin", "block", "validate", "packet-rules", "--directory", store)
        tested = run("plugin", "block", "test", "packet-rules", "--directory", store,
                     "--pcap", capture, "--set", "allowed_ether_types=[34998]", "--json")
        reports = [json.loads(line) for line in tested.splitlines()]
        if reports[0]["terminals"] != ["output:pass"] or reports[1]["terminals"] != ["output:drop"] or not reports[2]["rejection"]:
            raise ValueError("配布物のプラグインテスト結果が期待と異なります")
        run("plugin", "block", "watch", plugin, "--pcap", capture, "--once", "--json")
        traces = [directory / name for name in ("before.jsonl", "after.jsonl")]
        for trace in traces:
            trace.write_text(run("debug", "replay", "--config", ROOT / "amitoki.debug.example.toml",
                                 "--pcap", capture, "--directory", store, "--json") + "\n")
        run("debug", "compare", *traces)
        run("plugin", "block", "del", "packet-rules", "--directory", store)
        if (store / "packet-rules").exists() or not (plugin / "plugin.json").exists():
            raise ValueError("削除対象がインストール先に限定されていません")
    print(f"配布物の動作確認に成功: {binary}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--plugin", type=Path, required=True)
    arguments = parser.parse_args()
    smoke(arguments.binary.resolve(), arguments.plugin.resolve())


if __name__ == "__main__":
    main()
