"""版更新の範囲と、競合時にmainだけ・タグだけをpushしないことを確認する。"""
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts/release"))
import prepare


class VersionTests(unittest.TestCase):
    def test_older_equal_or_non_stable_versions_are_rejected(self):
        for value in ("0.1.0", "0.2.0", "v0.3.0", "01.2.3", "0.3.0-beta", "--help", "0.3.0\n"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                prepare.validate_version(value, "0.2.0")
        prepare.validate_version("0.10.0", "0.9.0")

    def test_only_the_core_package_version_changes(self):
        manifest = '[workspace]\nmembers = []\n\n[package]\nname = "amitoki"\nversion = "0.2.0"\n\n[dependencies]\nexample = "0.2.0"\n'
        updated = tomllib.loads(prepare.update_package_version(manifest, "0.3.0"))
        self.assertEqual(updated["package"]["version"], "0.3.0")
        self.assertEqual(updated["dependencies"]["example"], "0.2.0")
        lockfile = 'version = 4\n\n[[package]]\nname = "amitoki-packet"\nversion = "0.1.0"\n\n[[package]]\nname = "amitoki"\nversion = "0.2.0"\n\n[[package]]\nname = "amitoki-plugin-sdk"\nversion = "0.3.0"\n'
        updated = tomllib.loads(prepare.update_package_version(lockfile, "0.4.0", lockfile=True))
        self.assertEqual([package["version"] for package in updated["package"]], ["0.1.0", "0.4.0", "0.3.0"])


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "source"
        self.remote = Path(self.temporary.name) / "remote.git"
        self.root.mkdir()
        self.real_run = subprocess.run
        self.git("init", "--bare", str(self.remote))
        self.git("init", "--initial-branch=main", str(self.root))
        self.git("config", "user.name", "検証")
        self.git("config", "user.email", "test@example.invalid")
        (self.root / "docs/releases").mkdir(parents=True)
        (self.root / "Cargo.toml").write_text('[package]\nname = "amitoki"\nversion = "0.2.0"\n')
        (self.root / "Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "amitoki"\nversion = "0.2.0"\n')
        for name in ("docs/releases.md", "docs/amitoki-migration.md"):
            (self.root / name).write_text("本体v0.2.0\n")
        self.git("add", ".")
        self.git("commit", "-m", "検証用の初期状態")
        self.git("remote", "add", "origin", str(self.remote))
        self.git("push", "origin", "main")
        self.initial = self.git("rev-parse", "HEAD").stdout.strip()
        self.dispatches = []

    def git(self, *arguments):
        return self.real_run(["git", *arguments], cwd=self.root, text=True, capture_output=True, check=True)

    def run_command(self, command, **options):
        if command[0] == "gh":
            self.dispatches.append(command)
            return subprocess.CompletedProcess(command, 0)
        return self.real_run(command, **options, capture_output=True, text=True)

    def prepare(self, *, dry_run=False, runner=None):
        with patch.object(prepare, "ROOT", self.root), \
                patch.object(prepare, "prepare_preflight", return_value=(self.initial, "v0.3.0")), \
                patch.object(prepare, "read_or_generate_release_notes", return_value="# amitoki 0.3.0\n変更内容\n"), \
                patch.object(prepare.subprocess, "run", side_effect=runner or self.run_command):
            prepare.prepare_release("0.3.0", "example/amitoki", dry_run=dry_run)

    def test_dry_run_leaves_files_history_tags_and_workflows_unchanged(self):
        self.prepare(dry_run=True)
        self.assertEqual(self.git("status", "--porcelain").stdout, "")
        self.assertEqual(self.git("rev-parse", "HEAD").stdout.strip(), self.initial)
        self.assertEqual(self.git("tag", "--list").stdout, "")
        self.assertEqual(self.dispatches, [])

    def test_main_and_tag_point_to_the_same_prepared_commit_before_dispatch(self):
        self.prepare()
        remote_main = self.git("ls-remote", "origin", "refs/heads/main").stdout.split()[0]
        remote_tag = self.git("ls-remote", "origin", "refs/tags/v0.3.0^{}").stdout.split()[0]
        self.assertEqual(remote_main, remote_tag)
        self.assertNotEqual(remote_main, self.initial)
        self.assertEqual(tomllib.loads((self.root / "Cargo.toml").read_text())["package"]["version"], "0.3.0")
        self.assertEqual(self.git("status", "--porcelain").stdout, "")
        self.assertEqual(self.dispatches, [["gh", "workflow", "run", "rust.yml", "--repo", "example/amitoki", "--ref", "v0.3.0"]])

    def test_a_concurrent_main_update_prevents_both_tag_push_and_dispatch(self):
        tree = self.git("rev-parse", "HEAD^{tree}").stdout.strip()
        newer = self.git("commit-tree", tree, "-p", self.initial, "-m", "他の担当者による変更").stdout.strip()

        def race_before_push(command, **options):
            if command[:2] == ["git", "push"]:
                self.git("push", "origin", f"{newer}:refs/heads/main")
            return self.run_command(command, **options)

        with self.assertRaises(subprocess.CalledProcessError):
            self.prepare(runner=race_before_push)
        self.assertEqual(self.git("ls-remote", "origin", "refs/heads/main").stdout.split()[0], newer)
        self.assertEqual(self.git("ls-remote", "origin", "refs/tags/v0.3.0").stdout, "")
        self.assertEqual(self.dispatches, [])

    def test_a_mismatched_lockfile_stops_before_any_file_changes(self):
        (self.root / "Cargo.lock").write_text('[[package]]\nname = "amitoki"\nversion = "0.1.0"\n')
        before = (self.root / "Cargo.toml").read_bytes()
        with self.assertRaisesRegex(ValueError, "Cargo.lock"):
            prepare.render_release_files(self.root, "0.3.0", "notes")
        self.assertEqual((self.root / "Cargo.toml").read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
