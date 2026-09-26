"""本体の版・ターゲットと配布物名を一か所で決める。"""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
# ELF e_machineとDebian Architecture。配布物に別CPUの実行ファイルを混ぜない。
TARGETS = {"x86_64-unknown-linux-gnu": (62, "amd64"), "aarch64-unknown-linux-gnu": (183, "arm64")}


def output(*command, cwd=ROOT):
    return subprocess.check_output(command, cwd=cwd, text=True).strip()


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def version():
    with (ROOT / "Cargo.toml").open("rb") as stream:
        value = tomllib.load(stream)["package"]["version"]
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", value):
        raise ValueError("本体の配布は安定版のmajor.minor.patch形式を指定してください")
    return value


def filenames(release_version, target):
    architecture = TARGETS[target][1]
    return (f"amitoki-{release_version}-{target}.tar.gz", f"amitoki_{release_version}_{architecture}.deb")


def verify_binary(binary, target, release_version):
    with binary.open("rb") as stream:
        header = stream.read(20)
    if header[:6] != b"\x7fELF\x02\x01" or int.from_bytes(header[18:20], "little") != TARGETS[target][0]:
        raise ValueError(f"実行ファイルが{target}用の64bit ELFではありません")
    if output(str(binary.resolve()), "--version") != f"amitoki {release_version}":
        raise ValueError("実行ファイルとCargo.tomlの版が一致しません")


def build_metadata(binary, target):
    release_version = version()
    verify_binary(binary, target, release_version)
    return {
        "version": release_version,
        "target": target,
        "commit": output("git", "rev-parse", "HEAD"),
        "dirty": bool(output("git", "status", "--porcelain", "--untracked-files=normal")),
        "source_date_epoch": int(output("git", "show", "-s", "--format=%ct", "HEAD")),
        "rustc": output("rustc", "--version"),
        "sha256": digest(binary),
    }


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
