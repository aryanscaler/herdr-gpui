"""Exercise the installed shell entrypoint against a fake caller-local core."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SHIM = ROOT / "crates/sheprd-gpui/src/remote_browser/herdr-gpui.sh"
WORK = ROOT / "crates/sheprd-gpui/src/remote_work/shim.py"
FAKE = r'''#!/usr/bin/env python3
import json, os, pathlib, signal, sys
state = pathlib.Path(os.environ["FAKE_STATE"])
data = json.loads(state.read_text()) if state.exists() else {"labels":{},"writes":[]}
args = sys.argv[1:]
assert args[1] == "t1", args
if args[0] == "label":
    data["writes"].append(args[2:])
    for pair in args[2:]:
        name, value = pair.split("=", 1)
        if value:
            data["labels"][name] = value
        else:
            data["labels"].pop(name, None)
    state.write_text(json.dumps(data))
    print("{}")
elif args[0] == "get":
    mode = os.environ.get("FAKE_MODE", "ok")
    if data.get("interrupted"):
        print(json.dumps({"labels":data["labels"]}))
        sys.exit(0)
    key = next(iter(data["labels"]))
    request = json.loads(data["labels"][key])
    assert set(request) == {"version", "window", "operation"}
    reply = "sheprd-work-reply-" + key.removeprefix("sheprd-work-")
    if mode in ("interrupt", "claimed-interrupt"):
        if mode == "claimed-interrupt":
            data["labels"]["sheprd-work-claim-" + key.removeprefix("sheprd-work-")] = "1"
        data["interrupted"] = True
        state.write_text(json.dumps(data))
        os.kill(os.getppid(), signal.SIGTERM)
    elif mode == "cancel":
        data["labels"] = {}
    elif mode == "invalid":
        data["labels"][reply] = "not json"
    elif mode == "huge":
        data["labels"][reply] = "x" * 16385
    elif mode == "wrong-window":
        data["labels"][reply] = json.dumps({"status":"error", "code":"workspace_not_found", "message":"Window does not hold this remote terminal"})
    elif mode == "busy":
        data["labels"][reply] = json.dumps({"status":"error", "code":"window_busy", "message":"Close the dialog first"})
    else:
        op = request["operation"]
        if op["op"] in ("mute", "mute_status"):
            response = {"status":"work", "reply":{"kind":"mute", "mute":{"host":"SSH", "workspace":"tools", "muted":op.get("muted",False), "host_muted":False}}}
        else:
            response = {"status":"done", "message":"Opened the caller checkout dialog; confirm in Sheprd"}
        data["labels"][reply] = json.dumps(response)
    print(json.dumps({"labels":data["labels"]}))
else:
    raise AssertionError(args)
'''


class RemoteWork(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.state = self.root / "state.json"
        self.shim = self.root / "sheprd"
        self.shim.write_text("#!/bin/sh\ncore_work() {\nexec python3 - \"$@\" <<'SHEPRD_WORK_PY'\n" + WORK.read_text() + "\nSHEPRD_WORK_PY\n}\n" + SHIM.read_text())
        core = self.root / "herdr-core"
        core.write_text(FAKE)
        core.chmod(0o700)
        self.env = dict(os.environ, PATH=str(self.root) + os.pathsep + os.environ["PATH"], HC_TERMINAL_ID="t1", FAKE_STATE=str(self.state))
        self.env.pop("HERDR_WORKSPACE_ID", None)

    def run_work(self, *args, mode="ok"):
        return subprocess.run(["sh", str(self.shim), *args], env=dict(self.env, FAKE_MODE=mode), text=True, capture_output=True, timeout=5)

    def writes(self):
        return json.loads(self.state.read_text())["writes"]

    def assert_once_and_cleaned(self, writes=2):
        written = self.writes()
        self.assertEqual(len(written), writes, written)
        writes = written
        self.assertEqual(json.loads(self.state.read_text())["labels"], {})
        self.assertTrue(all(value.startswith("sheprd-work-") for row in writes for value in row))
        return json.loads(writes[0][0].split("=", 1)[1])

    def test_mute_roundtrip_keeps_the_host_and_window(self):
        result = self.run_work("--window", "17", "worktree", "mute", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(json.loads(result.stdout)["muted"])
        request = self.assert_once_and_cleaned()
        self.assertEqual(request["window"], 17)
        self.assertEqual(request["operation"], {"op":"mute", "muted":True})

    def test_dialog_requests_only_report_preparation(self):
        for args in [("scripts", "run"), ("fan-out", "fix bug", "--agent", "claude:2"),
                     ("worktree", "delete"), ("checkpoint", "restore", "123"),
                     ("teleport", "--to", "Box")]:
            with self.subTest(args=args):
                self.state.unlink(missing_ok=True)
                result = self.run_work(*args, "--json")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("confirm", json.loads(result.stdout)["message"])
                self.assert_once_and_cleaned()

    def test_refusals_and_malformed_responses_are_not_retried(self):
        for mode, status in [("wrong-window",5), ("busy",6), ("invalid",1), ("huge",1), ("cancel",1)]:
            with self.subTest(mode=mode):
                self.state.unlink(missing_ok=True)
                result = self.run_work("worktree", "mute", "--json", mode=mode)
                self.assertEqual(result.returncode, status, result.stderr)
                self.assert_once_and_cleaned()

    def test_an_interrupted_caller_withdraws_first_and_reports_a_claim_it_finds(self):
        for mode, claimed in [("interrupt", False), ("claimed-interrupt", True)]:
            with self.subTest(mode=mode):
                self.state.unlink(missing_ok=True)
                result = self.run_work("worktree", "mute", "--json", mode=mode)
                self.assertEqual(result.returncode, 130, result.stderr)
                # Request, withdrawal, then the usual cleanup of every label.
                self.assert_once_and_cleaned(writes=3)
                self.assertEqual(self.writes()[1], [self.writes()[0][0].split("=", 1)[0] + "="])
                self.assertEqual("already accepted" in result.stderr, claimed, result.stderr)

    def test_invalid_and_host_override_options_never_contact_core(self):
        for args in [("worktree", "delete", "--force"), ("scripts", "run", "--host", "elsewhere"),
                     ("worktree", "mute", "--window", "-1")]:
            result = self.run_work(*args, "--json")
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertFalse(self.state.exists())

    def test_core_terminal_machine_selector_is_refused(self):
        self.env["HC_TERMINAL_ID"] = "other/t1"
        result = self.run_work("worktree", "mute", "--json")
        self.assertEqual(result.returncode, 5)
        self.assertFalse(self.state.exists())

    def test_full_binary_only_commands_and_non_core_callers_return_json_refusals(self):
        for args in [("scripts", "list"), ("checkpoint", "list"), ("checkpoint", "diff", "123")]:
            result = self.run_work(*args, "--json")
            self.assertEqual(result.returncode, 8, result.stderr)
            self.assertEqual(json.loads(result.stdout)["error"]["code"], "unsupported")
        self.env["HERDR_WORKSPACE_ID"] = "w1"
        result = self.run_work("worktree", "mute", "--json")
        self.assertEqual(result.returncode, 8, result.stderr)
        self.assertEqual(json.loads(result.stdout)["error"]["code"], "unsupported")
        self.assertFalse(self.state.exists())


if __name__ == "__main__":
    unittest.main()
