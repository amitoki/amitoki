#!/usr/bin/env python3
"""mainへ取り込まれた版タグの検証済み配布物だけをGitHub Releasesへ公開する。"""
import argparse
import json
from pathlib import Path
import subprocess

from metadata import ROOT, TARGETS, digest, filenames, output, version
from verify import verify_target


def release_preflight(tag):
    release_version = version()
    if tag != f"v{release_version}":
        raise ValueError("タグとCargo.tomlの版が一致しません")
    commit = output("git", "rev-parse", "HEAD")
    if output("git", "rev-parse", f"refs/tags/{tag}^{{commit}}") != commit:
        raise ValueError("チェックアウトしたcommitとタグが一致しません")
    subprocess.run(["git", "merge-base", "--is-ancestor", commit, "origin/main"], cwd=ROOT, check=True)
    notes = ROOT / f"docs/releases/{tag}.md"
    if not notes.is_file():
        raise ValueError(f"リリースノートがありません: {notes}")
    return commit, notes


def publish(directory, tag, repository):
    commit, notes = release_preflight(tag)
    checksums = "".join(verify_target(directory, target, commit) for target in TARGETS)
    (directory / "SHA256SUMS").write_text(checksums)
    assets = [directory / name for target in TARGETS for name in filenames(version(), target)]
    assets.append(directory / "SHA256SUMS")
    base = ["gh", "release"]
    existing = subprocess.run([*base, "view", tag, "--repo", repository, "--json", "isDraft"], capture_output=True, text=True)
    if existing.returncode == 0:
        if not json.loads(existing.stdout)["isDraft"]:
            raise ValueError("公開済みReleaseは上書きしません")
        # 途中で失敗した自分のdraftにだけ再アップロードできる。
        subprocess.run([*base, "edit", tag, "--repo", repository, "--title", tag, "--notes-file", str(notes)], check=True)
    else:
        subprocess.run([*base, "create", tag, "--repo", repository, "--verify-tag", "--draft", "--title", tag, "--notes-file", str(notes)], check=True)
    subprocess.run([*base, "upload", tag, "--repo", repository, "--clobber", *map(str, assets)], check=True)
    # GitHubが返すdigestも照合してから公開し、部分的なアップロードを公開しない。
    # タグ指定のREST APIはdraftを返さない。CLIはdraftも解決して取得する。
    release = json.loads(output(*base, "view", tag, "--repo", repository, "--json", "assets"))
    expected = {path.name: f"sha256:{digest(path)}" for path in assets}
    uploaded = {asset["name"]: asset["digest"] for asset in release["assets"]}
    if uploaded != expected:
        raise ValueError("GitHub上の配布物がローカルと一致しません。draftのまま停止します")
    subprocess.run([*base, "edit", tag, "--repo", repository, "--draft=false", "--latest"], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--check-only", action="store_true")
    arguments = parser.parse_args()
    if arguments.check_only:
        commit, _ = release_preflight(arguments.tag)
        for target in TARGETS:
            verify_target(arguments.directory.resolve(), target, commit)
        print("リリースの事前検証に成功（公開は行っていません）")
    else:
        publish(arguments.directory.resolve(), arguments.tag, arguments.repository)


if __name__ == "__main__":
    main()
