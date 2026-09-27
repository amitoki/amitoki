"""配布物の取り違えと、不完全なReleaseの公開を防ぐ契約を確認する。"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts/release"))
import metadata
import package
import publish
import verify


class ArchiveTests(unittest.TestCase):
    def test_changing_local_timestamps_and_permissions_keeps_the_same_archive(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            tree = root / "bundle"
            tree.mkdir()
            (tree / "amitoki").write_bytes(b"binary")
            (tree / "readme.md").write_text("documentation")
            package.archive(tree, root / "first.tar.gz", 100)
            for path in tree.iterdir():
                os.utime(path, (1000, 1000))
                path.chmod(0o777)
            package.archive(tree, root / "second.tar.gz", 100)
            self.assertEqual(metadata.digest(root / "first.tar.gz"), metadata.digest(root / "second.tar.gz"))

    def test_symbolic_links_cannot_add_unlisted_files_to_an_archive(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            tree = root / "bundle"
            tree.mkdir()
            (tree / "escape").symlink_to("/etc/passwd")
            with self.assertRaises(ValueError):
                package.archive(tree, root / "archive.tar.gz", 100)

    def test_a_different_cpu_is_rejected_before_executing_the_binary(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / "amitoki"
            binary.write_bytes(b"\x7fELF\x02\x01" + bytes(12) + (183).to_bytes(2, "little"))
            with patch.object(metadata, "output") as execute, self.assertRaises(ValueError):
                metadata.verify_binary(binary, "x86_64-unknown-linux-gnu", metadata.version())
            execute.assert_not_called()

    def test_tampered_distribution_stops_verification(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            target = "x86_64-unknown-linux-gnu"
            names = metadata.filenames(metadata.version(), target)
            for name in names:
                (directory / name).write_bytes(b"tampered")
            (directory / f"SHA256SUMS-{target}.txt").write_text("original checksums")
            with self.assertRaisesRegex(ValueError, "SHA256"):
                verify.verify_target(directory, target, None)


class PublishingTests(unittest.TestCase):
    def test_version_mismatch_stops_before_git_or_github_calls(self):
        with patch.object(publish, "output") as execute, self.assertRaises(ValueError):
            publish.release_preflight("v999.0.0")
        execute.assert_not_called()

    def test_a_tag_outside_main_is_rejected(self):
        with patch.object(publish, "output", return_value="commit"), \
                patch.object(publish.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "git")), \
                self.assertRaises(subprocess.CalledProcessError):
            publish.release_preflight(f"v{metadata.version()}")

    def test_published_releases_are_never_overwritten(self):
        with tempfile.TemporaryDirectory() as temporary, \
                patch.object(publish, "release_preflight", return_value=("commit", Path("notes"))), \
                patch.object(publish, "verify_target", return_value=""), \
                patch.object(publish, "digest", return_value="hash"), \
                patch.object(publish.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, '{"isDraft":false,"assets":[]}')) as run:
            with self.assertRaisesRegex(ValueError, "公開済み"):
                publish.publish(Path(temporary), f"v{metadata.version()}", "example/repository")
            self.assertEqual(run.call_count, 1)
            self.assertEqual(run.call_args.args[0][:3], ["gh", "release", "view"])

    def test_an_identical_published_release_is_not_modified_on_retry(self):
        names = [name for target in metadata.TARGETS for name in metadata.filenames(metadata.version(), target)] + ["SHA256SUMS"]
        published = {"isDraft": False, "body": "release notes", "assets": [{"name": name, "digest": "sha256:hash"} for name in names]}
        with tempfile.TemporaryDirectory() as temporary:
            notes = Path(temporary) / "notes.md"
            notes.write_text("release notes\n")
            with patch.object(publish, "release_preflight", return_value=("commit", notes)), \
                    patch.object(publish, "verify_target", return_value=""), \
                    patch.object(publish, "digest", return_value="hash"), \
                    patch.object(publish.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, json.dumps(published))) as run:
                publish.publish(Path(temporary), f"v{metadata.version()}", "example/repository")
                self.assertEqual(run.call_count, 1)
                self.assertEqual(run.call_args.args[0][:3], ["gh", "release", "view"])

    def test_incomplete_upload_remains_a_draft(self):
        with tempfile.TemporaryDirectory() as temporary, \
                patch.object(publish, "release_preflight", return_value=("commit", Path("notes"))), \
                patch.object(publish, "verify_target", return_value=""), \
                patch.object(publish, "digest", return_value="hash"), \
                patch.object(publish, "output", return_value=json.dumps({"assets": []})), \
                patch.object(publish.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, '{"isDraft":true}')) as run:
            with self.assertRaisesRegex(ValueError, "draft"):
                publish.publish(Path(temporary), f"v{metadata.version()}", "example/repository")
            commands = [call.args[0] for call in run.call_args_list]
            self.assertTrue(any("upload" in command for command in commands))
            self.assertFalse(any("--draft=false" in command for command in commands))

    def test_only_a_complete_matching_upload_is_published(self):
        names = [name for target in metadata.TARGETS for name in metadata.filenames(metadata.version(), target)] + ["SHA256SUMS"]
        uploaded = {"assets": [{"name": name, "digest": "sha256:hash"} for name in names]}

        def read_draft(*command):
            if command[:2] == ("gh", "api"):
                raise subprocess.CalledProcessError(1, command, stderr="Not Found (HTTP 404)")
            return json.dumps(uploaded)

        with tempfile.TemporaryDirectory() as temporary, \
                patch.object(publish, "release_preflight", return_value=("commit", Path("notes"))), \
                patch.object(publish, "verify_target", return_value=""), \
                patch.object(publish, "digest", return_value="hash"), \
                patch.object(publish, "output", side_effect=read_draft), \
                patch.object(publish.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, '{"isDraft":true}')) as run:
            publish.publish(Path(temporary), f"v{metadata.version()}", "example/repository")
            self.assertIn("--draft=false", run.call_args.args[0])


if __name__ == "__main__":
    unittest.main()
