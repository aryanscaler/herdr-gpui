"""Nix builds cover bundle inputs and approved contributor pull requests."""

import fnmatch
import itertools
from pathlib import Path
import re
import unittest

import test_ci_policy as policy


ROOT = Path(__file__).resolve().parents[3]
WORKFLOW = (ROOT / ".github/workflows/nix.yml").read_text()
JOBS = {"build": WORKFLOW.split("\njobs:\n  build:\n", 1)[1]}


class NixWorkflowTests(unittest.TestCase):
    def test_bundle_input_changes_trigger_both_events(self):
        inputs = [
            "flake.nix", "flake.lock", "Cargo.lock", "rust-toolchain.toml",
            "LICENSE", "NOTICE", "crates/sheprd-gpui/SOUND-NOTICE.md",
            "scripts/release/herdr-core.json", "scripts/release/herdr-core-licenses.txt",
            ".github/workflows/nix.yml",
        ]
        for directory in ("assets/macos", "assets/icons", "macos/sheprd-nearby"):
            inputs.extend(path.relative_to(ROOT).as_posix() for path in (ROOT / directory).rglob("*")
                          if path.is_file())
        for event in ("pull_request", "push"):
            section = re.search(rf"^  {event}:\n((?:    .*\n)+)", WORKFLOW, re.M)[1]
            patterns = re.findall(r"^      - (.+)$", section, re.M)
            for path in inputs:
                with self.subTest(event=event, path=path):
                    self.assertTrue(any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns))

    def test_contributor_pull_requests_run_in_the_upstream_repository(self):
        for repository, actor in itertools.product(
                ("sheprd-labs/sheprd", "penso/herdr-gpui", "other/sheprd"),
                ("contributor", "penso")):
            context = policy.CiPolicy().context(**{
                "github.repository": repository,
                "github.triggering_actor": actor,
            })
            self.assertEqual(policy.condition_matches("build", context, JOBS), repository != "other/sheprd")

    def test_non_pr_builds_keep_the_owner_main_boundary(self):
        for event, ref, actor, triggering_actor in itertools.product(
                ("push", "workflow_dispatch", "pull_request_target", "workflow_run"),
                ("refs/heads/main", "refs/heads/topic"),
                ("penso", "contributor"), ("penso", "contributor")):
            context = policy.CiPolicy().context(**{
                "github.event_name": event, "github.ref": ref,
                "github.actor": actor, "github.triggering_actor": triggering_actor,
            })
            expected = (event in ("push", "workflow_dispatch") and ref == "refs/heads/main"
                        and actor == triggering_actor == "penso")
            self.assertEqual(policy.condition_matches("build", context, JOBS), expected)

    def test_pr_builds_have_no_persisted_credentials_or_secrets(self):
        self.assertIn("\npermissions: {}\n", WORKFLOW)
        self.assertIn("\n      contents: read\n", JOBS["build"])
        self.assertIn("persist-credentials: false", JOBS["build"])
        for forbidden in ("secrets.", "secrets[", "secrets:", "environment:", "id-token:"):
            self.assertNotIn(forbidden, JOBS["build"])
