"""公開ファイルの不足・取り違え・別URL・改ざんを検出する。"""
import copy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts/release"))
import verify_published
from metadata import TARGETS, filenames


class PublishedReleaseTests(unittest.TestCase):
    def setUp(self):
        self.expected = {"tag": "v0.2.0", "repository": "example/amitoki", "notes": "release notes"}
        names = [name for target in TARGETS for name in filenames("0.2.0", target)] + ["SHA256SUMS"]
        self.release = {"tag_name": "v0.2.0", "draft": False, "prerelease": False, "body": "release notes\n",
                        "assets": [{"name": name, "digest": "sha256:" + "a" * 64,
                                    "browser_download_url": "https://github.com/example/amitoki/releases/download/v0.2.0/" + name}
                                   for name in names]}

    def test_a_complete_release_with_matching_notes_is_accepted(self):
        verify_published.validate_release(self.release, self.expected)

    def test_unpublished_wrong_tag_and_different_notes_are_rejected(self):
        for field, value in (("draft", True), ("prerelease", True), ("tag_name", "v0.1.0"), ("body", "wrong notes")):
            with self.subTest(field=field), self.assertRaises(ValueError):
                verify_published.validate_release({**self.release, field: value}, self.expected)

    def test_missing_duplicate_and_unexpected_assets_are_rejected(self):
        assets = self.release["assets"]
        for replacement in (assets[1:], assets + [assets[0]], assets[1:] + [assets[-1]]):
            with self.assertRaises(ValueError):
                verify_published.validate_release({**self.release, "assets": replacement}, self.expected)

    def test_external_urls_and_missing_digests_are_rejected(self):
        for field, value in (("browser_download_url", "https://other.invalid/file"), ("digest", None), ("digest", "sha256:wrong")):
            changed = copy.deepcopy(self.release)
            changed["assets"][0][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                verify_published.validate_release(changed, self.expected)


if __name__ == "__main__":
    unittest.main()
