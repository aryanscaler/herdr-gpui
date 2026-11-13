"""Exercise the installed POSIX shim against a deterministic core CLI peer."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest


SHIM = Path(__file__).resolve().parents[2] / "crates/sheprd-gpui/src/remote_browser/herdr-gpui.sh"
PEER = r'''
import json, os, pathlib, sys
home = pathlib.Path(os.environ["HOME"])
path = home / "labels.json"
labels = json.loads(path.read_text()) if path.exists() else {}
args = sys.argv[1:]
assert args[1] == "t3", args
with (home / "calls.jsonl").open("a") as out:
    out.write(json.dumps(args) + "\n")
if args[0] == "label":
    for pair in args[2:]:
        key, value = pair.split("=", 1)
        if value:
            labels[key] = value
        else:
            labels.pop(key, None)
    path.write_text(json.dumps(labels))
elif args[0] == "get":
    mode = os.environ.get("MODE", "notes")
    if mode == "disconnect":
        sys.exit(1)
    if mode == "oversized":
        print("x" * (1024 * 1024 + 1))
        sys.exit(0)
    for key, value in list(labels.items()):
        if value.startswith("feedback-"):
            reply = "sheprd-reply-" + key[len("herdr-gpui-"):]
            if mode == "cancel":
                labels.pop(key)
            elif mode != "interrupt" and (mode != "delayed" or (home / "polled").exists()):
                labels[reply] = json.dumps({"notes": None if mode == "empty" else "Remote café\nsecond line"})
    (home / "polled").touch()
    path.write_text(json.dumps(labels))
    print(json.dumps({"address": "t3", "state": "live", "labels": labels}))
'''


class FeedbackProtocol(unittest.TestCase):
    def run_shim(self, mode="notes", args=()):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            binary = home / ".local/bin/herdr-core"
            binary.parent.mkdir(parents=True)
            binary.write_text(f"#!{sys.executable}\n" + PEER)
            binary.chmod(0o755)
            env = {"HOME": directory, "PATH": os.path.dirname(sys.executable) + ":/usr/bin:/bin",
                   "HC_TERMINAL_ID": "t3", "MODE": mode}
            command = ["/bin/sh", str(SHIM), "browser", "feedback", "--json", *args]
            if mode == "interrupt":
                child = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                try:
                    deadline = time.monotonic() + 3
                    while not (home / "polled").exists():
                        if time.monotonic() >= deadline or child.poll() is not None:
                            self.fail("feedback caller did not reach its read loop")
                        time.sleep(0.01)
                    child.terminate()
                    stdout, stderr = child.communicate(timeout=3)
                    result = subprocess.CompletedProcess(command, child.returncode, stdout, stderr)
                finally:
                    if child.poll() is None:
                        child.kill()
                    child.wait(timeout=3)
            else:
                result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=5)
            calls = home / "calls.jsonl"
            labels = home / "labels.json"
            return (result, [json.loads(line) for line in calls.read_text().splitlines()] if calls.exists() else [],
                    json.loads(labels.read_text()) if labels.exists() else {})

    def test_returns_unicode_multiline_notes_and_acknowledges_exact_request(self):
        result, calls, labels = self.run_shim(args=("--wait", "600"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), {"notes": "Remote café\nsecond line"})
        key = calls[0][2].split("=", 1)[0]
        self.assertEqual(calls[0][2], key + "=feedback-600")
        self.assertEqual(calls[-1], ["label", "t3", key + "=ack"])
        self.assertEqual(labels[key], "ack")

    def test_empty_reply_uses_cli_no_notes_exit_status(self):
        result, _, _ = self.run_shim("empty")
        self.assertEqual(result.returncode, 4, result.stderr)
        self.assertEqual(json.loads(result.stdout), {"notes": None})

    def test_wait_collects_a_later_reply_without_resubmitting(self):
        result, calls, _ = self.run_shim("delayed", args=("--wait", "1"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(sum(call[0] == "get" for call in calls), 2)
        self.assertEqual(sum(any("=feedback-" in arg for arg in call) for call in calls), 1)

    def test_disconnect_and_cancellation_do_not_acknowledge_notes(self):
        for mode in ("disconnect", "cancel"):
            with self.subTest(mode=mode):
                result, calls, labels = self.run_shim(mode)
                self.assertEqual(result.returncode, 1)
                self.assertIn("error", json.loads(result.stdout))
                self.assertFalse(any(arg.endswith("=ack") for call in calls for arg in call))
                self.assertEqual(labels, {})

    def test_invalid_wait_never_contacts_core(self):
        for wait in ("601", "-1", "NaN"):
            result, calls, _ = self.run_shim(args=("--wait", wait))
            self.assertEqual(result.returncode, 2)
            self.assertEqual(calls, [])

    def test_terminating_the_caller_cancels_its_request_without_ack(self):
        result, calls, labels = self.run_shim("interrupt", args=("--wait", "600"))
        self.assertEqual(result.returncode, 130, result.stderr)
        self.assertEqual(labels, {})
        self.assertFalse(any(arg.endswith("=ack") for call in calls for arg in call))

    def test_oversized_core_response_is_refused_and_cleaned(self):
        result, calls, labels = self.run_shim("oversized")
        self.assertEqual(result.returncode, 1)
        self.assertIn("1 MiB", json.loads(result.stdout)["error"]["message"])
        self.assertEqual(labels, {})
        self.assertFalse(any(arg.endswith("=ack") for call in calls for arg in call))


if __name__ == "__main__":
    unittest.main()
