#!/usr/bin/env python3
"""全CPUの配布物をチェックし、公開用SHA256SUMSを作る。実行は行わない。"""
import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile

from metadata import TARGETS, digest, filenames, output, version


def archive_contents(path):
    with tarfile.open(path, "r:gz") as archive:
        contents = {}
        for member in archive.getmembers():
            name = PurePosixPath(member.name)
            if name.is_absolute() or ".." in name.parts or not (member.isfile() or member.isdir()):
                raise ValueError(f"不正なアーカイブ項目: {member.name}")
            if member.isfile():
                contents[member.name] = archive.extractfile(member).read()
        return contents


def verify_target(directory, target, commit):
    release_version = version()
    names = filenames(release_version, target)
    expected = "".join(f"{digest(directory / name)}  {name}\n" for name in names)
    if (directory / f"SHA256SUMS-{target}.txt").read_text() != expected:
        raise ValueError(f"{target}: SHA256が一致しません")
    contents = archive_contents(directory / names[0])
    prefix = names[0].removesuffix(".tar.gz")
    metadata = json.loads(contents[f"{prefix}/build-info.json"])
    if metadata["target"] != target or metadata["version"] != release_version:
        raise ValueError("ビルド情報の版・CPUが一致しません")
    if commit and (metadata["commit"] != commit or metadata["dirty"]):
        raise ValueError("配布物は指定commitのクリーンなビルドではありません")
    if hashlib.sha256(contents[f"{prefix}/amitoki"]).hexdigest() != metadata["sha256"]:
        raise ValueError("実行ファイルのSHA256がビルド情報と一致しません")
    deb = directory / names[1]
    for field, expected_value in [("Package", "amitoki"), ("Version", release_version), ("Architecture", TARGETS[target][1])]:
        if output("dpkg-deb", "-f", str(deb), field) != expected_value:
            raise ValueError(f"debの{field}が一致しません")
    deb_tar = subprocess.check_output(["dpkg-deb", "--fsys-tarfile", str(deb)])
    with tarfile.open(fileobj=io.BytesIO(deb_tar)) as archive:
        binary = archive.extractfile("./usr/bin/amitoki").read()
        build_info = archive.extractfile("./usr/share/doc/amitoki/build-info.json").read()
    if binary != contents[f"{prefix}/amitoki"] or json.loads(build_info) != metadata:
        raise ValueError("debとアーカイブの実行ファイル・ビルド情報が一致しません")
    return expected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--target", choices=TARGETS, action="append")
    parser.add_argument("--commit", help="指定commitからのクリーンなビルドに限定する")
    arguments = parser.parse_args()
    checksums = "".join(verify_target(arguments.directory.resolve(), target, arguments.commit)
                        for target in (arguments.target or TARGETS))
    (arguments.directory / "SHA256SUMS").write_text(checksums)
    print(checksums, end="")


if __name__ == "__main__":
    main()
