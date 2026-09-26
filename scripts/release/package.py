#!/usr/bin/env python3
"""本体の実行ファイルアーカイブとdebを同じビルドから作る（Python 3.11以上）。"""
import argparse
import gzip
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

from metadata import ROOT, TARGETS, build_metadata, digest, filenames, output, write_json

# 配布対象を明示し、ローカル設定・VMの鍵・プラグインの接続情報を取り込まない。
DOCUMENTS = ["readme.md", "docs"]
EXAMPLES = ["amitoki.example.toml", "amitoki.pipeline.example.toml", "amitoki.debug.example.toml"]
MAINTAINER = "相田 優希 <51500566+aida0710@users.noreply.github.com>"


def copy_documentation(destination):
    destination.mkdir(parents=True, exist_ok=True)
    for name in DOCUMENTS:
        source = ROOT / name
        if source.is_dir():
            shutil.copytree(source, destination / name)
        else:
            shutil.copyfile(source, destination / name)
    for name in EXAMPLES:
        shutil.copyfile(ROOT / name, destination / name)


def archive(package, destination, epoch):
    # gzipヘッダ、tarの時刻・権限・所有者をビルド環境に依存させない。
    with destination.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=epoch) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as bundle:
                for path in [package, *sorted(package.rglob("*"))]:
                    if path.is_symlink():
                        raise ValueError(f"配布物にシンボリックリンクは使えません: {path}")
                    member = bundle.gettarinfo(str(path), arcname=str(path.relative_to(package.parent)))
                    member.uid = member.gid = 0
                    member.uname = member.gname = ""
                    member.mtime = epoch
                    member.mode = 0o755 if path.is_dir() or path.name == "amitoki" else 0o644
                    if path.is_file():
                        with path.open("rb") as stream:
                            bundle.addfile(member, stream)
                    else:
                        bundle.addfile(member)


def debian_package(package, destination, metadata):
    stage = package.parent / "debian-package"
    executable = stage / "usr/bin/amitoki"
    executable.parent.mkdir(parents=True)
    shutil.copyfile(package / "amitoki", executable)
    executable.chmod(0o755)
    documentation = stage / "usr/share/doc/amitoki"
    shutil.copytree(package / "share", documentation)
    shutil.copyfile(package / "build-info.json", documentation / "build-info.json")
    # shlibdepsを実際のELFへ適用し、glibc/libgccの要求版を推測しない。
    source_control = stage / "debian/control"
    source_control.parent.mkdir()
    source_control.write_text(f"Source: amitoki\nMaintainer: {MAINTAINER}\n\nPackage: amitoki\nArchitecture: any\nDescription: Network analysis and relay tool\n")
    dependencies = output("dpkg-shlibdeps", "-O", f"-e{executable}", cwd=stage).removeprefix("shlibs:Depends=")
    shutil.rmtree(source_control.parent)
    control = stage / "DEBIAN/control"
    control.parent.mkdir()
    installed_size = (sum(path.stat().st_size for path in stage.rglob("*") if path.is_file()) + 1023) // 1024
    control.write_text(
        f"Package: amitoki\nVersion: {metadata['version']}\nArchitecture: {TARGETS[metadata['target']][1]}\n"
        f"Maintainer: {MAINTAINER}\nSection: net\nPriority: optional\nInstalled-Size: {installed_size}\n"
        f"Depends: {dependencies}, ca-certificates\nHomepage: https://github.com/amitoki/amitoki\n"
        "Description: Network analysis, packet debugging and pluggable relays\n"
        " Build packet pipelines from external block and relay plugins.\n"
        " Includes offline PCAP debugging and a plugin development watcher.\n",
        encoding="utf-8",
    )
    epoch = metadata["source_date_epoch"]
    for path in stage.rglob("*"):
        path.chmod(0o755 if path.is_dir() or path == executable else 0o644)
        os.utime(path, (epoch, epoch))
    os.utime(stage, (epoch, epoch))
    subprocess.run(["dpkg-deb", "--root-owner-group", "-Zxz", "--build", str(stage), str(destination)],
                   env={**os.environ, "SOURCE_DATE_EPOCH": str(epoch)}, check=True)


def package_release(binary, target, destination):
    metadata = build_metadata(binary, target)
    destination.mkdir(parents=True, exist_ok=True)
    names = filenames(metadata["version"], target)
    with tempfile.TemporaryDirectory(prefix="amitoki-release-") as temporary:
        package = Path(temporary) / names[0].removesuffix(".tar.gz")
        package.mkdir()
        shutil.copyfile(binary, package / "amitoki")
        (package / "amitoki").chmod(0o755)
        copy_documentation(package / "share")
        write_json(package / "build-info.json", metadata)
        archive(package, destination / names[0], metadata["source_date_epoch"])
        debian_package(package, destination / names[1], metadata)
    checksums = "".join(f"{digest(destination / name)}  {name}\n" for name in names)
    (destination / f"SHA256SUMS-{target}.txt").write_text(checksums)
    print(checksums, end="")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/amitoki")
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "dist/release")
    arguments = parser.parse_args()
    package_release(arguments.binary.resolve(), arguments.target, arguments.output.resolve())


if __name__ == "__main__":
    main()
