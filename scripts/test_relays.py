#!/usr/bin/env python3
"""Tests for scripts/relays.py, which edits Sheprd's signed relay list."""

import json
import os
import subprocess
import sys
import tempfile
import unittest

SCRIPT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "relays.py")


def run(*args):
    return subprocess.run(
        [sys.executable, SCRIPT, *args], capture_output=True, text=True, check=False
    )


class RelaysTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.directory.name, "relays.json")
        with open(self.path, "w", encoding="utf-8") as file:
            json.dump({"version": 1, "relays": []}, file)

    def tearDown(self):
        self.directory.cleanup()

    def read(self):
        with open(self.path, encoding="utf-8") as file:
            return json.load(file)

    def test_every_change_stamps_a_newer_serial_and_an_expiry(self):
        self.assertEqual(run("stamp", self.path).returncode, 0)
        first = self.read()
        self.assertGreater(first["expires"], first["serial"])
        self.assertEqual(run("stamp", self.path, "--days", "30").returncode, 0)
        second = self.read()
        self.assertGreater(second["serial"], first["serial"])
        self.assertLess(second["expires"], first["expires"])

    def test_relays_are_added_once_and_removed(self):
        pin = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA"
        for _ in range(2):
            added = run("add", self.path, "fra1.relay.example.org/", "--region", "fra1",
                        "--name", "Frankfurt", "--pin", pin)
            self.assertEqual(added.returncode, 0, added.stderr)
        relays = self.read()["relays"]
        self.assertEqual(relays, [{
            "endpoint": "https://fra1.relay.example.org",
            "region": "fra1",
            "name": "Frankfurt",
            "quic_pin": pin,
        }])
        self.assertEqual(run("remove", self.path, "https://fra1.relay.example.org").returncode, 0)
        self.assertEqual(self.read()["relays"], [])
        self.assertNotEqual(run("remove", self.path, "https://fra1.relay.example.org").returncode, 0)

    def test_plain_http_is_refused(self):
        added = run("add", self.path, "http://relay.example.org", "--region", "x", "--name", "x",
                    "--no-pin")
        self.assertNotEqual(added.returncode, 0)


if __name__ == "__main__":
    unittest.main()
