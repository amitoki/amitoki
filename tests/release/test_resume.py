"""公開の再開で別commitや未検証の配布物を使わないことを確認する。"""
import copy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts/release"))
import resume
from metadata import TARGETS


class ResumeTests(unittest.TestCase):
    def setUp(self):
        self.expected = {"tag": "v0.1.0", "commit": "commit", "repository": "example/repository"}
        self.run = {"path": ".github/workflows/rust.yml", "event": "push",
                    "head_branch": "v0.1.0", "head_sha": "commit", "status": "completed",
                    "repository": {"full_name": "example/repository"}, "conclusion": "failure"}
        names = ["Lint and isolated network tests", *[f"Test and package ({target})" for target in TARGETS]]
        self.jobs = [{"name": name, "conclusion": "success"} for name in names]
        self.jobs.append({"name": "Publish version tag", "conclusion": "failure"})

    def test_a_failed_publish_can_resume_after_all_validation_jobs_succeed(self):
        resume.verify_build_run(self.run, self.jobs, self.expected)

    def test_other_sources_or_unfinished_builds_are_rejected(self):
        cases = {"path": "other.yml", "event": "pull_request", "head_branch": "main",
                 "head_sha": "different", "status": "in_progress",
                 "repository": {"full_name": "other/repository"}}
        for field, value in cases.items():
            with self.subTest(field=field), self.assertRaises(ValueError):
                resume.verify_build_run({**self.run, field: value}, self.jobs, self.expected)

    def test_missing_failed_skipped_or_duplicate_validation_jobs_are_rejected(self):
        for conclusion in ("failure", "skipped", "cancelled", None):
            jobs = copy.deepcopy(self.jobs)
            jobs[1]["conclusion"] = conclusion
            with self.subTest(conclusion=conclusion), self.assertRaises(ValueError):
                resume.verify_build_run(self.run, jobs, self.expected)
        for jobs in (self.jobs[1:], [*self.jobs, self.jobs[0]]):
            with self.assertRaises(ValueError):
                resume.verify_build_run(self.run, jobs, self.expected)


if __name__ == "__main__":
    unittest.main()
