#!/usr/bin/env python3
"""Compares how fast Sheprd builds draw one workload, frame by frame.

Each app runs in turn against its own private Herdr daemon (a fresh sandbox
with its own HOME, XDG folders, config, and sockets; the user's daemon is
never touched), with one repository and `--trees` linked worktrees. The
workload has phases: idle, steady terminal output in the shown pane, agents
changing state, then focusing each workspace in turn. GPUI times every frame
itself (`ZED_MEASUREMENTS=1`); this reads those lines and the app's CPU time.

Rounds alternate the apps, so load from the rest of the machine spreads over
both, and the table shows the median of the rounds. It opens real windows,
so it needs an active desktop.

    scripts/bench_frames.py --herdr /path/to/herdr \
        main=target/release/sheprd moonframe=target/moonframe/release/sheprd
"""

import argparse
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

# Frames over this miss a 60 Hz display's refresh.
BUDGET_MS = 16.7
# How long a focus may take to settle before its frames count as another's.
FOCUS_WINDOW_S = 0.4
# Seconds between focusing one workspace and the next.
FOCUS_GAP_S = 0.45
# The most focus changes measured per run.
FOCUS_COUNT = 24
# Seconds the app gets to connect and draw its first frames.
SETTLE_S = 8
# Seconds a daemon gets to open its socket.
DAEMON_WAIT_S = 15
# What the app needs to reach the desktop on Linux (X11 or Wayland), kept
# from the caller's environment for the app alone.
DESKTOP = ("DISPLAY", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "XAUTHORITY", "XDG_SESSION_TYPE")

UNITS = {"ns": 1e-6, "µs": 1e-3, "us": 1e-3, "ms": 1.0, "s": 1000.0}
FRAME = re.compile(r"frame duration: ([\d.]+)(ns|µs|us|ms|s)\b")

OUTPUT_SCRIPT = (
    "i=0; while [ $i -lt 100000 ]; do i=$((i+1)); "
    'echo "line $i of a build log with some text in it to fill the row up"; '
    "sleep 0.01; done"
)


class Sandbox:
    """A private daemon and the environment both it and the app run in."""

    def __init__(self, herdr: str, trees: int):
        parent = os.environ.get("HERDR_TEST_TMPDIR") or "/tmp"
        # Short, as Unix socket paths are limited to about 100 bytes.
        self.root = Path(tempfile.mkdtemp(prefix="sb-", dir=parent))
        self.herdr = herdr
        home = self.root / "home"
        home.mkdir()
        self.env = {
            "HOME": str(home),
            "PATH": "/usr/bin:/bin",
            "SHELL": "/bin/sh",
            "TERM": "xterm-256color",
            "LANG": "en_US.UTF-8",
            "HERDR_CONFIG_PATH": str(self.root / "config.toml"),
            "HERDR_SOCKET_PATH": str(self.root / "a.sock"),
            "HERDR_CLIENT_SOCKET_PATH": str(self.root / "c.sock"),
            "XDG_CONFIG_HOME": str(self.root / "x/c"),
            "XDG_STATE_HOME": str(self.root / "x/s"),
            "XDG_DATA_HOME": str(self.root / "x/d"),
            "XDG_CACHE_HOME": str(self.root / "x/h"),
        }
        self.daemon = subprocess.Popen(
            [herdr, "server"],
            env=self.env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        deadline = time.monotonic() + DAEMON_WAIT_S
        while not (self.root / "a.sock").exists():
            if time.monotonic() > deadline or self.daemon.poll() is not None:
                self.close()
                raise SystemExit(f"{herdr} server did not start")
            time.sleep(0.2)
        self.panes = self._populate(home, trees)

    def app_env(self) -> dict:
        """The private environment, plus what a GUI needs to reach the
        desktop: the app gets these, the daemon never does."""
        desktop = {name: os.environ[name] for name in DESKTOP if name in os.environ}
        return {**self.env, **desktop}

    def run(self, *args: str) -> dict:
        done = subprocess.run(
            [self.herdr, *args], env=self.env, capture_output=True, text=True
        )
        try:
            return json.loads(done.stdout).get("result", {})
        except json.JSONDecodeError:
            return {}

    def _populate(self, home: Path, trees: int) -> list:
        repo = home / "demo"
        repo.mkdir()
        git = ["git", "-C", str(repo)]
        subprocess.run([*git, "init", "-q", "-b", "main"], check=True)
        subprocess.run(
            [*git, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q",
             "--allow-empty", "-m", "first"],
            check=True,
        )
        self.run("workspace", "create", "--cwd", str(repo))
        workspace = self.workspaces()[0]
        for i in range(1, trees + 1):
            self.run(
                "worktree", "create", "--workspace", workspace,
                "--branch", f"agent/feature-number-{i}-with-a-long-branch-name",
                "--label", f"feature {i} with a long label",
                "--path", str(home / f"w{i}"), "--no-focus", "--trust-repository",
            )
        panes = [p["pane_id"] for p in self.run("pane", "list").get("panes", [])]
        self.report(panes, "idle", "waiting")
        return panes

    def workspaces(self) -> list:
        listed = self.run("workspace", "list").get("workspaces", [])
        return [w["workspace_id"] for w in listed]

    def report(self, panes: list, state: str, message: str) -> None:
        for pane in panes:
            self.run(
                "pane", "report-agent", pane, "--source", "bench",
                "--agent", "claude", "--state", state, "--message", message,
            )

    def close(self) -> None:
        stop(self.daemon)
        shutil.rmtree(self.root, ignore_errors=True)


def stop(process: subprocess.Popen) -> None:
    """Ends one child this script started, and waits for it."""
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def cpu_seconds(pid: int) -> float:
    out = subprocess.run(
        ["ps", "-o", "time=", "-p", str(pid)], capture_output=True, text=True
    ).stdout.strip()
    total = 0.0
    for part in out.split(":"):
        total = total * 60 + float(part or 0)
    return total


def percentile(values: list, p: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(len(ordered) * p))]


def run_once(herdr: str, binary: str, trees: int, phase_s: float) -> dict:
    """One app through the whole workload: a row of numbers per phase."""
    sandbox = Sandbox(herdr, trees)
    frames = []
    app = None
    try:
        app = subprocess.Popen(
            [binary],
            env={**sandbox.app_env(), "ZED_MEASUREMENTS": "1"},
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            errors="replace",
        )

        def read() -> None:
            for line in app.stdout:
                match = FRAME.search(line)
                if match:
                    frames.append((time.time(), float(match[1]) * UNITS[match[2]]))

        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        time.sleep(SETTLE_S)
        if app.poll() is not None:
            raise SystemExit(f"{binary} exited during startup")

        rows = {}

        def phase(name: str, during=None, after=None) -> None:
            if during:
                during()
            time.sleep(1)
            start, cpu0 = time.time(), cpu_seconds(app.pid)
            time.sleep(phase_s)
            end, cpu1 = time.time(), cpu_seconds(app.pid)
            if after:
                after()
            inside = [ms for at, ms in frames if start <= at < end]
            seconds = end - start
            rows[name] = {
                "fps": len(inside) / seconds,
                "p50": percentile(inside, 0.5),
                "p95": percentile(inside, 0.95),
                "max": max(inside, default=0.0),
                "slow": sum(ms > BUDGET_MS for ms in inside),
                "cpu": (cpu1 - cpu0) / seconds * 100,
            }

        shown = sandbox.panes[0]
        phase("idle")
        phase(
            "output",
            during=lambda: sandbox.run("pane", "run", shown, OUTPUT_SCRIPT),
            after=lambda: sandbox.run("pane", "send-keys", shown, "C-c"),
        )
        churning = threading.Event()

        def churn() -> None:
            step = 0
            while not churning.is_set():
                step += 1
                sandbox.report(sandbox.panes[:1], "working", f"step {step}")
                time.sleep(0.2)

        churner = threading.Thread(target=churn, daemon=True)
        phase("churn", during=churner.start, after=churning.set)
        churner.join()
        sandbox.report(sandbox.panes, "idle", "waiting")
        time.sleep(1)

        focused = []
        ids = sandbox.workspaces()
        for i in range(FOCUS_COUNT):
            sandbox.run("workspace", "focus", ids[i % len(ids)])
            focused.append(time.time())
            time.sleep(FOCUS_GAP_S)
        # From the daemon having the focus to the last frame it caused.
        settle = []
        for at in focused:
            burst = [t for t, _ in frames if at - 0.03 <= t < at + FOCUS_WINDOW_S]
            if burst:
                settle.append((burst[-1] - at) * 1000)
        rows["focus"] = {
            "settle_p50": percentile(settle, 0.5),
            "settle_p95": percentile(settle, 0.95),
        }
        return rows
    finally:
        if app is not None:
            stop(app)
        sandbox.close()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--herdr", required=True, help="the herdr binary to start")
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--trees", type=int, default=14, help="linked worktrees")
    parser.add_argument("--phase", type=float, default=8, help="seconds per phase")
    parser.add_argument("--json", action="store_true", help="print each run as JSON")
    parser.add_argument("apps", nargs="+", metavar="NAME=BINARY")
    args = parser.parse_args()

    apps = []
    for spec in args.apps:
        name, _, binary = spec.partition("=")
        if not binary or not Path(binary).is_file():
            parser.error(f"{spec}: expected NAME=BINARY naming a built binary")
        apps.append((name, str(Path(binary).resolve())))

    runs = {name: [] for name, _ in apps}
    for round_ in range(1, args.rounds + 1):
        for name, binary in apps:
            print(f"round {round_}/{args.rounds}: {name}", file=sys.stderr)
            rows = run_once(args.herdr, binary, args.trees, args.phase)
            runs[name].append(rows)
            if args.json:
                print(json.dumps({"app": name, "round": round_, **rows}))

    load = os.getloadavg()[0]
    print(f"\nMedian of {args.rounds} rounds (load average now {load:.1f})\n")
    metrics = [
        ("idle", "cpu", "idle CPU %"),
        ("output", "p50", "output frame p50 ms"),
        ("output", "p95", "output frame p95 ms"),
        ("output", "max", "output frame max ms"),
        ("output", "fps", "output fps"),
        ("output", "slow", f"output frames over {BUDGET_MS}ms"),
        ("output", "cpu", "output CPU %"),
        ("churn", "p50", "churn frame p50 ms"),
        ("churn", "p95", "churn frame p95 ms"),
        ("churn", "cpu", "churn CPU %"),
        ("focus", "settle_p50", "focus settle p50 ms"),
        ("focus", "settle_p95", "focus settle p95 ms"),
    ]
    width = max(len(label) for _, _, label in metrics)
    print(" " * width + "".join(f"{name:>12}" for name, _ in apps))
    for phase, key, label in metrics:
        cells = "".join(
            f"{statistics.median(r[phase][key] for r in runs[name]):>12.2f}"
            for name, _ in apps
        )
        print(f"{label:<{width}}{cells}")


if __name__ == "__main__":
    main()
