#!/usr/bin/env python3
"""検証済みタグのCI配布物を使い、更新した公開処理でReleaseを再開する。"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from metadata import ROOT, TARGETS, output


def verify_build_run(run, jobs, expected):
    if (run["path"] != ".github/workflows/rust.yml" or run["event"] != "push"
            or run["head_branch"] != expected["tag"] or run["head_sha"] != expected["commit"]
            or run["repository"]["full_name"] != expected["repository"]
            or run["status"] != "completed"):
        raise ValueError("指定runは対象タグのCIではないか、完了していません")
    required = {"Lint and isolated network tests"}
    required.update(f"Test and package ({target})" for target in TARGETS)
    for name in required:
        matching = [job for job in jobs if job["name"] == name]
        if len(matching) != 1 or matching[0]["conclusion"] != "success":
            raise ValueError(f"必須の検証が成功していません: {name}")


def download_distributions(run_id, repository, directory):
    command = ["gh", "run", "download", str(run_id), "--repo", repository, "--dir", str(directory)]
    for target in TARGETS:
        command.extend(["--name", f"release-{target}"])
    subprocess.run(command, check=True)
    # 複数ArtifactのダウンロードはArtifact名のサブディレクトリを作る。
    for target in TARGETS:
        artifact = directory / f"release-{target}"
        for path in artifact.iterdir():
            destination = directory / path.name
            if not path.is_file() or destination.exists():
                raise ValueError(f"想定外の配布物: {path.name}")
            shutil.move(path, destination)
        artifact.rmdir()


def resume_release(tag, run_id, repository):
    # 版・mainへの包含・配布物のcommitは、タグ側の情報でpublish.pyが再確認する。
    commit = output("git", "rev-parse", f"refs/tags/{tag}^{{commit}}")
    endpoint = f"repos/{repository}/actions/runs/{run_id}"
    run = json.loads(output("gh", "api", endpoint))
    pages = json.loads(output("gh", "api", "--paginate", "--slurp", f"{endpoint}/jobs"))
    jobs = [job for page in pages for job in page["jobs"]]
    verify_build_run(run, jobs, {"tag": tag, "commit": commit, "repository": repository})

    with tempfile.TemporaryDirectory(prefix="amitoki-release-resume-") as temporary:
        directory = Path(temporary)
        distributions = directory / "distributions"
        download_distributions(run_id, repository, distributions)

        source = directory / "source"
        subprocess.run(["git", "worktree", "add", "--detach", str(source), commit], cwd=ROOT, check=True)
        try:
            # タグと配布物は固定し、mainで修正した検証・公開スクリプトだけを使う。
            for name in ("metadata.py", "verify.py", "publish.py"):
                shutil.copyfile(ROOT / "scripts/release" / name, source / "scripts/release" / name)
            subprocess.run([sys.executable, str(source / "scripts/release/publish.py"),
                            "--directory", str(distributions), "--tag", tag,
                            "--repository", repository], cwd=source, check=True)
        finally:
            subprocess.run(["git", "worktree", "remove", "--force", str(source)], cwd=ROOT, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--run-id", type=int, required=True)
    parser.add_argument("--repository", required=True)
    arguments = parser.parse_args()
    resume_release(arguments.tag, arguments.run_id, arguments.repository)


if __name__ == "__main__":
    main()
