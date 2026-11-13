"""Managed remote work CLI. Only the caller's core is contacted; never retry a mutation."""
import json
import os
import secrets
import selectors
import shutil
import signal
import subprocess
import sys
import time
import unicodedata

MAX_REQUEST = 4096
MAX_REPLY = 16384
EXIT = {"usage": 2, "invalid_url": 2, "not_found": 5, "workspace_not_found": 5,
        "no_window": 5, "window_busy": 6, "busy": 6, "tab_limit": 6,
        "timeout": 7, "unsupported": 8}


def printable(text):
    return "".join(char for char in str(text) if char in "\n\t" or unicodedata.category(char) not in ("Cc", "Cf"))


class Unsupported(ValueError):
    pass


def parse(arguments):
    args, window, as_json = [], None, False
    iterator = iter(arguments)
    for arg in iterator:
        if arg == "--json":
            as_json = True
        elif arg == "--window" or arg.startswith("--window="):
            value = next(iterator, "") if arg == "--window" else arg.split("=", 1)[1]
            if not value.isascii() or not value.isdigit() or int(value) > 2**64 - 1:
                raise ValueError("--window needs a window ID")
            window = int(value)
        else:
            args.append(arg)
    if not args:
        raise ValueError("Give a work command")
    family, args = args[0], args[1:]
    if (family == "scripts" and (not args or args[0] == "list")) or (family == "checkpoint" and (not args or args[0] in ("list", "create", "diff"))):
        raise Unsupported("This command needs the full Sheprd CLI binary on the host")
    if family == "worktree":
        verb = args.pop(0) if args else "status"
        if verb == "create" and len(args) <= 1:
            operation = {"op": "kept_create", "branch": args[0] if args else None}
            args = []
        elif verb in ("list", "open", "delete"):
            operation = {"op": "kept_" + verb}
        elif verb in ("mute", "unmute"):
            operation = {"op": "mute", "muted": verb == "mute"}
        elif verb == "status":
            operation = {"op": "mute_status"}
        else:
            raise ValueError("Unknown worktree command")
    elif family == "notes":
        verb = args.pop(0) if args else "list"
        if verb == "set" and args:
            operation = {"op": "set_note", "text": " ".join(args)}
            args = []
        elif verb == "clear":
            operation = {"op": "set_note", "text": ""}
        elif verb in ("list", "get"):
            operation = {"op": "notes" if verb == "list" else "note"}
        else:
            raise ValueError("Use notes list|get|set TEXT|clear")
    elif family == "scripts" and args and args[0] in ("run", "setup", "archive"):
        operation = {"op": "script", "kind": args.pop(0)}
    elif family == "checkpoint" and args and args.pop(0) == "restore" and len(args) <= 1:
        checkpoint = args.pop(0) if args else None
        if checkpoint is not None and (not checkpoint.isascii() or not checkpoint.isdigit() or len(checkpoint) > 32):
            raise ValueError("Invalid checkpoint ID")
        operation = {"op": "restore", "id": checkpoint}
    elif family == "teleport" and len(args) == 2 and args[0] == "--to":
        operation = {"op": "teleport", "host": args[1]}
        args = []
    elif family == "fan-out":
        words, agents = [], []
        iterator = iter(args)
        for arg in iterator:
            if arg == "--agent" or arg.startswith("--agent="):
                value = next(iterator, "") if arg == "--agent" else arg.split("=", 1)[1]
                kind, _, count = value.partition(":")
                count = count or "1"
                if not kind or not count.isascii() or not count.isdigit() or not 1 <= int(count) <= 8:
                    raise ValueError("--agent needs KIND[:COUNT], with count 1..8")
                agents.append([kind.lower(), int(count)])
            elif arg.startswith("--"):
                raise ValueError("Unknown option: " + arg)
            else:
                words.append(arg)
        if not words:
            raise ValueError("Give a fan-out prompt")
        operation = {"op": "fan_out", "prompt": " ".join(words), "agents": agents}
        args = []
    else:
        raise ValueError("Remote work supports notes, worktree, scripts setup/run, checkpoint restore, teleport --to, and fan-out")
    if args:
        raise ValueError("Unexpected arguments: " + " ".join(args))
    request = json.dumps({"version": 1, "window": window, "operation": operation}, ensure_ascii=False)
    if len(request.encode()) > MAX_REQUEST:
        raise ValueError("Remote work request exceeds 4096 bytes")
    return request, as_json


def output(response, as_json):
    if not isinstance(response, dict):
        raise ValueError("Unexpected remote work response")
    status = response.get("status")
    if status == "error":
        status = EXIT.get(response["code"], 1)
        code = {2:"usage", 5:"not_found", 6:"busy", 7:"timeout", 8:"unsupported"}.get(status, "refused")
        result = {"error": {"code": code, "message": response["message"]}}
        print(json.dumps(result) if as_json else printable(response["message"]),
              file=sys.stdout if as_json else sys.stderr, flush=True)
        return status
    if status == "done":
        result = {"message": response["message"]}
        text = response["message"]
    elif status == "work":
        reply = response["reply"]
        kind = reply["kind"]
        if kind == "mute":
            result = reply["mute"]
            text = result["workspace"] + (" is muted" if result["muted"] else " is not muted")
            if result["host_muted"]:
                text += "; its host is muted"
        elif kind == "note":
            result = {"note": reply["note"]}
            text = reply["note"]["text"] if reply["note"] else "This checkout has no note"
        elif kind in ("notes", "worktrees"):
            result = {key: value for key, value in reply.items() if key != "kind"}
            text = json.dumps(result, ensure_ascii=False)
        else:
            raise ValueError("Unexpected work reply")
    else:
        raise ValueError("Unexpected remote work response")
    reply = response.get("reply") if status == "work" else None
    # Exit 4 means nothing to show, as the full CLI reports it.
    nothing = reply is not None and (
        (reply["kind"] == "note" and reply["note"] is None)
        or (reply["kind"] == "notes" and not reply["notes"] and not reply["omitted"])
    )
    if nothing and reply["kind"] == "notes":
        text = "No worktree notes"
    print(json.dumps(result) if as_json else printable(text), flush=True)
    return 4 if nothing else 0


def run(arguments):
    if "--help" in arguments or "-h" in arguments:
        print("""Remote Herdr 2 work commands [--json] [--window ID]:
  worktree [status|mute|unmute|list|create [BRANCH]|open|delete]
  notes [list|get|set TEXT|clear]   (list is restricted to this host)
  scripts setup|run              (unseen scripts ask in Sheprd)
  checkpoint restore [ID]
  teleport --to HOST
  fan-out PROMPT [--agent KIND[:COUNT]]...
Dialogs only prepare work; confirm in Sheprd. A request waits at most 30s.
After timeout or cancellation check the app before retrying: no retry is sent.
Requires python3 and a window driving this terminal on its saved host.
Checkpoint reads/diffs and scripts list need the full CLI on this host.""")
        return 0
    as_json = "--json" in arguments
    try:
        request, as_json = parse(arguments)
    except Unsupported as error:
        return output({"status": "error", "code": "unsupported", "message": str(error)}, as_json)
    except ValueError as error:
        return output({"status": "error", "code": "usage", "message": str(error)}, as_json)
    terminal = os.environ.get("HC_TERMINAL_ID", "")
    # Do not let a forged machine/name selector retarget hc away from this core.
    if not terminal or "/" in terminal or any(char.isspace() for char in terminal) or terminal.startswith("-"):
        return output({"status": "error", "code": "workspace_not_found", "message": "No local HC_TERMINAL_ID"}, as_json)
    core = shutil.which("herdr-core") or os.path.expanduser("~/.local/bin/herdr-core")
    nonce = secrets.token_hex(16)
    key, reply = "sheprd-work-" + nonce, "sheprd-work-reply-" + nonce
    claim = "sheprd-work-claim-" + nonce
    deadline = time.monotonic() + 30

    def call(*args, cleanup=False):
        command = [core, *args]
        until = time.monotonic() + 2 if cleanup else min(deadline, time.monotonic() + 10)
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL) as child:
            try:
                data = bytearray()
                with selectors.DefaultSelector() as ready:
                    ready.register(child.stdout, selectors.EVENT_READ)
                    while True:
                        remaining = until - time.monotonic()
                        if remaining <= 0 or not ready.select(remaining):
                            raise subprocess.TimeoutExpired(command, 10)
                        chunk = os.read(child.stdout.fileno(), 65536)
                        if not chunk:
                            break
                        data.extend(chunk)
                        if len(data) > 1024 * 1024:
                            raise ValueError("Core response exceeds 1 MiB")
                status = child.wait(timeout=max(0.1, until - time.monotonic()))
                if status:
                    raise subprocess.CalledProcessError(status, command)
                return bytes(data)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=2)

    def interrupted(signum, frame):
        raise KeyboardInterrupt

    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, interrupted)
    try:
        call("label", terminal, key + "=" + request)
        while time.monotonic() < deadline:
            record = json.loads(call("get", terminal, "--json"))
            labels = record["labels"]
            if reply in labels:
                text = labels[reply]
                if not isinstance(text, str) or len(text.encode()) > MAX_REPLY:
                    raise ValueError("Remote work reply exceeds its limit")
                return output(json.loads(text), as_json)
            if key not in labels or record.get("state") == "finished":
                raise ValueError("Request cancelled or terminal ended; do not retry ambiguous work")
            time.sleep(0.2)
        return output({"status": "error", "code": "timeout", "message": "No matching Sheprd window answered. Check the app before retrying; work may have been prepared."}, as_json)
    except KeyboardInterrupt:
        # Withdraw first: a window that has not claimed yet then never will.
        # A claim already written means the work may run; say so.
        try:
            call("label", terminal, key + "=", cleanup=True)
            if claim in json.loads(call("get", terminal, "--json", cleanup=True))["labels"]:
                print("Sheprd already accepted this request; check the app before retrying.", file=sys.stderr)
        except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
            pass
        return 130
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        return output({"status": "error", "code": "failed", "message": str(error) + "; no retry was sent"}, as_json)
    finally:
        try:
            call("label", terminal, key + "=", reply + "=", claim + "=", cleanup=True)
        except (OSError, ValueError, subprocess.SubprocessError):
            pass


if __name__ == "__main__":
    sys.exit(run(sys.argv[1:]))
