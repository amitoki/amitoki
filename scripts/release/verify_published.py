#!/usr/bin/env python3
"""公開された配布物を認証なしで取得し、タグの内容とSHA256を確認する。"""
import argparse
import json
from pathlib import Path
import re
import shutil
import urllib.request

from metadata import ROOT, TARGETS, digest, filenames, output, version, write_json
from verify import verify_target

# CDNの取得が止まっても公開ジョブ全体の期限を消費しない。
DOWNLOAD_TIMEOUT_SECONDS = 60


def open_public_url(url):
    # CIにGH_TOKENがあっても利用せず、利用者と同じ公開経路を通す。
    return urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "amitoki-release-verification"}),
                                  timeout=DOWNLOAD_TIMEOUT_SECONDS)


def validate_release(release, expected):
    tag, repository, notes = expected["tag"], expected["repository"], expected["notes"]
    release_version = tag.removeprefix("v")
    expected_names = {name for target in TARGETS for name in filenames(release_version, target)} | {"SHA256SUMS"}
    assets = release["assets"]
    if release["tag_name"] != tag or release["draft"] or release["prerelease"]:
        raise ValueError("対象の正式Releaseではありません")
    if release["body"].strip() != notes.strip():
        raise ValueError("公開されたリリースノートがタグと一致しません")
    if len(assets) != len(expected_names) or {asset["name"] for asset in assets} != expected_names:
        raise ValueError("公開配布物の不足・重複・想定外のファイルがあります")
    prefix = f"https://github.com/{repository}/releases/download/{tag}/"
    for asset in assets:
        if asset["browser_download_url"] != prefix + asset["name"]:
            raise ValueError("対象Release以外のダウンロードURLです")
        if not re.fullmatch(r"sha256:[a-f0-9]{64}", asset["digest"] or ""):
            raise ValueError("公開配布物のSHA256がありません")


def verify_published(tag, repository, directory):
    if tag != f"v{version()}":
        raise ValueError("タグとCargo.tomlの版が一致しません")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("リポジトリはowner/repo形式で指定してください")
    commit = output("git", "rev-parse", f"refs/tags/{tag}^{{commit}}")
    if output("git", "rev-parse", "HEAD") != commit:
        raise ValueError("検証は対象タグのcheckoutから実行してください")
    with open_public_url(f"https://api.github.com/repos/{repository}/releases/tags/{tag}") as response:
        release = json.load(response)
    notes = (ROOT / f"docs/releases/{tag}.md").read_text()
    validate_release(release, {"tag": tag, "repository": repository, "notes": notes})
    directory.mkdir(parents=True, exist_ok=True)
    for asset in release["assets"]:
        path = directory / asset["name"]
        with open_public_url(asset["browser_download_url"]) as response, path.open("wb") as destination:
            shutil.copyfileobj(response, destination)
        if "sha256:" + digest(path) != asset["digest"]:
            raise ValueError(f"取得した配布物のSHA256が一致しません: {path.name}")
    # verify_targetと同じCPU別チェックサムを公開の単一ファイルから再構成する。
    checksums = (directory / "SHA256SUMS").read_text().splitlines(keepends=True)
    expected_checksums = []
    for target in TARGETS:
        names = filenames(version(), target)
        selected = [line for line in checksums if len(line.split()) == 2 and line.split()[1] in names]
        (directory / f"SHA256SUMS-{target}.txt").write_text("".join(selected))
        expected_checksums.extend(verify_target(directory, target, commit).splitlines())
    if sorted(line.rstrip() for line in checksums) != sorted(expected_checksums):
        raise ValueError("公開チェックサムの一覧が配布物と一致しません")
    report = {"status": "passed", "tag": tag, "commit": commit,
              "anonymous_download": True, "assets": {asset["name"]: asset["digest"] for asset in release["assets"]}}
    write_json(directory / "verification.json", report)
    print(f"{tag}: 両CPUの公開配布物・SHA256・タグ・リリースノートを匿名取得で確認")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--directory", type=Path, required=True)
    arguments = parser.parse_args()
    verify_published(arguments.tag, arguments.repository, arguments.directory.resolve())


if __name__ == "__main__":
    main()
