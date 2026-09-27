#!/usr/bin/env python3
"""mainの版更新と注釈タグをまとめてpushし、タグのCIを明示的に起動する。"""
import argparse
import difflib
import json
from pathlib import Path
import re
import subprocess
import tomllib

from metadata import ROOT, output, version

# 安定版だけを扱い、GitのrefやCLIオプションとして解釈される入力を拒否する。
VERSION_PATTERN = r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
REPOSITORY_PATTERN = r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+"
AUTHOR = ["-c", "user.name=github-actions[bot]",
          "-c", "user.email=41898282+github-actions[bot]@users.noreply.github.com"]


def validate_version(release_version, current_version):
    if not re.fullmatch(VERSION_PATTERN, release_version):
        raise ValueError("版はvなしのmajor.minor.patchで指定してください")
    if tuple(map(int, release_version.split("."))) <= tuple(map(int, current_version.split("."))):
        raise ValueError("現在より新しい版を指定してください。作成済みタグのCIは再実行で再開できます")


def update_package_version(source, release_version, *, lockfile=False):
    header = r"\[\[package\]\]" if lockfile else r"\[package\]"
    sections = re.finditer(rf"(?ms)^{header}\n.*?(?=^\[|\Z)", source)
    for section in sections:
        if re.search(r'^name = "amitoki"$', section[0], re.MULTILINE):
            changed, count = re.subn(r'^version = "[^"]+"$', f'version = "{release_version}"',
                                    section[0], count=1, flags=re.MULTILINE)
            if count != 1:
                raise ValueError("本体のversionがありません")
            return source[:section.start()] + changed + source[section.end():]
    raise ValueError("本体のpackageがありません")


def render_release_files(root, release_version, notes):
    manifest = (root / "Cargo.toml").read_text()
    current_version = tomllib.loads(manifest)["package"]["version"]
    validate_version(release_version, current_version)
    lockfile = (root / "Cargo.lock").read_text()
    packages = [package for package in tomllib.loads(lockfile)["package"] if package["name"] == "amitoki"]
    if len(packages) != 1 or packages[0]["version"] != current_version:
        raise ValueError("Cargo.tomlとCargo.lockの本体版が一致しません")
    if not notes.strip():
        raise ValueError("リリースノートが空です")
    changes = {
        "Cargo.toml": update_package_version(manifest, release_version),
        "Cargo.lock": update_package_version(lockfile, release_version, lockfile=True),
        f"docs/releases/v{release_version}.md": notes.rstrip() + "\n",
    }
    # 最新版を指すインストール手順だけを更新し、過去のリリースノートは保持する。
    changes["docs/releases.md"] = (root / "docs/releases.md").read_text().replace(current_version, release_version)
    changes["docs/amitoki-migration.md"] = (root / "docs/amitoki-migration.md").read_text().replace(
        f"本体v{current_version}", f"本体v{release_version}")
    return changes


def prepare_preflight(release_version, repository):
    validate_version(release_version, version())
    if not re.fullmatch(REPOSITORY_PATTERN, repository):
        raise ValueError("リポジトリはowner/repo形式で指定してください")
    if output("git", "status", "--porcelain"):
        raise ValueError("作業ツリーに変更があります")
    commit = output("git", "rev-parse", "HEAD")
    if commit != output("git", "rev-parse", "origin/main"):
        raise ValueError("最新のorigin/mainから開始してください")
    tag = f"v{release_version}"
    if output("git", "ls-remote", "origin", f"refs/tags/{tag}"):
        raise ValueError("同じ版のタグが既にあります。上書きしません")
    runs = json.loads(output("gh", "run", "list", "--repo", repository, "--workflow", "rust.yml",
                             "--limit", "100", "--json", "headBranch,status"))
    if any(run["status"] != "completed" and re.fullmatch("v" + VERSION_PATTERN, run["headBranch"]) for run in runs):
        raise ValueError("別のリリースCIが進行中です。完了後に開始してください")
    return commit, tag


def read_or_generate_release_notes(repository, tag, commit):
    existing = ROOT / f"docs/releases/{tag}.md"
    if existing.is_file():
        return existing.read_text()
    generated = json.loads(output("gh", "api", f"repos/{repository}/releases/generate-notes",
                                  "--method", "POST", "-f", f"tag_name={tag}",
                                  "-f", f"target_commitish={commit}"))
    return f"# amitoki {tag.removeprefix('v')}\n\n{generated['body']}\n"


def write_summary(tag, changes, *, dry_run):
    summary = f"{'試行' if dry_run else 'リリースCIを開始'}: {tag}\n\n"
    summary += "\n".join(f"- {name}" for name in changes) + "\n"
    summary += "\n公開の成否はCI and releaseのタグ実行で確認してください。\n" if not dry_run else "\ncommit・タグ・公開は変更していません。\n"
    if dry_run:
        differences = []
        for name, content in changes.items():
            path = ROOT / name
            previous = path.read_text() if path.exists() else ""
            differences.extend(difflib.unified_diff(previous.splitlines(keepends=True), content.splitlines(keepends=True),
                                                     fromfile=name, tofile=name))
        summary += "\n````diff\n" + "".join(differences) + "````\n"
    print(summary)
    return summary


def prepare_release(release_version, repository, *, dry_run=False):
    commit, tag = prepare_preflight(release_version, repository)
    changes = render_release_files(ROOT, release_version, read_or_generate_release_notes(repository, tag, commit))
    if not dry_run:
        for name, content in changes.items():
            (ROOT / name).write_text(content)
        subprocess.run(["git", "add", "--", *changes], cwd=ROOT, check=True)
        subprocess.run(["git", *AUTHOR, "commit", "-m", f"chore(release): 本体{tag}の公開を準備"], cwd=ROOT, check=True)
        subprocess.run(["git", *AUTHOR, "tag", "-a", tag, "-m", f"amitoki {tag}を公開"], cwd=ROOT, check=True)
        # mainが他の変更で進んだ場合は、タグも含めて失敗させる。force pushは使わない。
        subprocess.run(["git", "push", "--atomic", "origin", "HEAD:refs/heads/main", f"refs/tags/{tag}"], cwd=ROOT, check=True)
        # GITHUB_TOKENのpushはCIを起動しないため、dispatchで明示的に接続する。
        subprocess.run(["gh", "workflow", "run", "rust.yml", "--repo", repository, "--ref", tag], cwd=ROOT, check=True)
    return write_summary(tag, changes, dry_run=dry_run)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--summary", type=Path)
    arguments = parser.parse_args()
    summary = prepare_release(arguments.version, arguments.repository, dry_run=arguments.dry_run)
    if arguments.summary:
        with arguments.summary.open("a") as stream:
            stream.write(summary)


if __name__ == "__main__":
    main()
