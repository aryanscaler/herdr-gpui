#!/bin/sh
# herdr-gpui-managed-shim v1: Sheprd keeps this file up to date. Edit a copy under another name to keep your changes.
#
# Sheprd's `herdr-gpui` command for an agent on an SSH host. Sheprd runs
# on the user's own machine, which this host cannot reach, so a request is
# left as metadata on the caller's Herdr workspace, for a short while, or, in
# a Herdr 2 (herdr-core) terminal, as labels on that terminal, which the
# window removes once it acts; a Sheprd window connected to this host opens
# the page in the workspace that shows the caller.
set -u
LC_ALL=C
export LC_ALL

# Herdr keeps at most 80 characters per token, 16 tokens per report, and 32
# per workspace, so a request is a header and at most 12 parts of its address.
CHUNK=80
CHUNKS=12
TTL_MS=30000

json=0

usage() {
    cat <<'EOF'
Usage: sheprd browser open [--no-focus] [--tab] URL
       sheprd browser reload
       sheprd browser feedback [--wait SECONDS]

Run on an SSH host, this asks the Sheprd window connected to this host to
open URL in a browser tab of your workspace (HERDR_WORKSPACE_ID), or, in a
Herdr 2 terminal (HC_TERMINAL_ID), of the workspace that shows it.

  open URL      An http or https address, or a bare host such as
                localhost:3000. An address on this host's localhost is
                forwarded to the user's machine first.
  --no-focus    Add the tab without switching to it.
  --tab         Accepted: a page from this host always opens as a tab.
  reload        Reload the tabs this pane opened.
  feedback      Herdr 2: return notes sent to this terminal (exit 4 if none).
  --wait N      Wait up to N seconds for notes (0..600); requires python3.
  --json        Print one JSON document.

Files on this host cannot be shown: serve
their folder (for example `python3 -m http.server 8000`) and open
localhost:8000. Herdr 1 notes arrive as your next prompt. In Herdr 2, collect
notes with browser feedback --wait 600 (or feedback --wait 600).

Herdr 2 callers can also use worktree, notes, scripts setup/run, checkpoint
restore, teleport, and fan-out with Python 3. Run COMMAND --help for usage.
These return scoped results; consequential work still waits for the app's
confirmation. After timeout/cancellation inspect the app before retrying.

Exit statuses: 0 done/asked, 1 refused, 2 invalid usage, 4 no notes,
5 caller/window not found, 6 busy, 7 timeout, 8 unsupported.
EOF
}

json_string() {
    printf '"%s"' "$(printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g')"
}

fail() {
    status=$1
    code=$2
    shift 2
    if [ "$json" = 1 ]; then
        printf '{"error":{"code":"%s","message":%s}}\n' "$code" "$(json_string "$*")"
    else
        printf '%s\n' "$*" >&2
    fi
    exit "$status"
}

done_message() {
    if [ "$json" = 1 ]; then
        printf '{"message":%s}\n' "$(json_string "$*")"
    else
        printf '%s\n' "$*"
    fi
}

unavailable() {
    fail 8 unsupported "Not available through this remote shim; see --help for browser and Herdr 2 work commands."
}

# In a herdr-core terminal outside a Herdr pane: the core, as the bridge
# finds it.
core_terminal() {
    [ -z "${HERDR_WORKSPACE_ID:-}" ] && [ -n "${HC_TERMINAL_ID:-}" ]
}

# send_core ACTION [URL]: the request as the calling terminal's labels, set at
# once, under the same names a workspace's tokens have.
send_core() {
    action=$1
    url=${2:-}
    core=$(command -v herdr-core 2>/dev/null || :)
    [ -n "$core" ] || core="$HOME/.local/bin/herdr-core"
    [ -x "$core" ] || fail 5 not_found "herdr-core is not on PATH or in ~/.local/bin."
    key="herdr-gpui-$(date +%s)-$$"
    set -- "$key=$action"
    rest=$url
    i=0
    while [ -n "$rest" ] && [ "$i" -lt "$CHUNKS" ]; do
        chunk=$(printf '%s\n' "$rest" | cut -c "1-$CHUNK")
        rest=$(printf '%s\n' "$rest" | cut -c "$((CHUNK + 1))-")
        set -- "$@" "${key}_$i=$chunk"
        i=$((i + 1))
    done
    "$core" label "$HC_TERMINAL_ID" "$@" >/dev/null ||
        fail 1 refused "herdr-core refused the request."
}

# send ACTION [URL]: one report, so a window sees the whole request at once.
# The request's ID names its tokens, so requests from this or another pane
# made before a window reads them stand side by side.
send() {
    if core_terminal; then
        send_core "$@"
        return
    fi
    action=$1
    url=${2:-}
    workspace=${HERDR_WORKSPACE_ID:-}
    [ -n "$workspace" ] || fail 5 not_found "Not running in a Herdr pane: HERDR_WORKSPACE_ID is not set."
    herdr=${HERDR_BIN_PATH:-herdr}
    key="herdr-gpui-$(date +%s)-$$"
    header=$action
    if [ -n "${HERDR_PANE_ID:-}" ]; then
        header="$header $HERDR_PANE_ID"
    fi
    set -- --source herdr-gpui --ttl-ms "$TTL_MS" --token "$key=$header"
    rest=$url
    i=0
    while [ -n "$rest" ] && [ "$i" -lt "$CHUNKS" ]; do
        chunk=$(printf '%s\n' "$rest" | cut -c "1-$CHUNK")
        rest=$(printf '%s\n' "$rest" | cut -c "$((CHUNK + 1))-")
        set -- "$@" --token "${key}_$i=$chunk"
        i=$((i + 1))
    done
    "$herdr" workspace report-metadata "$workspace" "$@" >/dev/null ||
        fail 1 refused "Herdr refused the request; is this a Herdr pane, or are many requests from it still waiting?"
}

open_page() {
    focus=1
    target=
    for arg in "$@"; do
        case $arg in
            --no-focus) focus=0 ;;
            # A page from a host always opens as a tab.
            --tab) ;;
            --beside) fail 8 unsupported "A page opened from an SSH host opens as a tab; --beside is not available here." ;;
            -*) fail 2 usage "Unknown option: $arg" ;;
            *)
                [ -z "$target" ] || fail 2 usage "Give one address."
                target=$arg
                ;;
        esac
    done
    [ -n "$target" ] || fail 2 usage "Give an address to open."
    case $target in
        *://*) ;;
        *)
            if [ -e "$target" ]; then
                fail 8 unsupported "Sheprd cannot show files from an SSH host. Serve the folder, for example with \`python3 -m http.server 8000\`, and open localhost:8000."
            fi
            ;;
    esac
    case $target in
        *://*)
            case $target in
                http://* | https://*) ;;
                *) fail 2 usage "Only http and https addresses open." ;;
            esac
            ;;
    esac
    case $target in
        *[!!-~]*) fail 2 usage "An address has no spaces or control characters." ;;
    esac
    [ "${#target}" -le $((CHUNK * CHUNKS)) ] ||
        fail 2 usage "Addresses longer than $((CHUNK * CHUNKS)) characters cannot be sent from an SSH host."
    if [ "$focus" = 1 ]; then
        send open "$target"
    else
        send open-background "$target"
    fi
    if core_terminal; then
        done_message "Asked Sheprd to open $target; a window showing this terminal opens it."
    else
        done_message "Asked Sheprd to open $target in workspace $HERDR_WORKSPACE_ID; a window connected to this host opens it."
    fi
}

reload_pages() {
    [ "$#" = 0 ] || fail 2 usage "browser reload takes no arguments."
    core_terminal || [ -n "${HERDR_PANE_ID:-}" ] || fail 1 refused "browser reload runs in the pane that opened the page."
    send reload
    done_message "Asked Sheprd to reload this pane's browser tabs."
}

# The caller only talks to its own core. No desktop socket, port, or server is
# exposed on this host. A reply is acknowledged only after stdout is flushed;
# interrupted/failed readers leave the notes available for a later request.
core_feedback() {
    core_terminal || unavailable
    command -v python3 >/dev/null 2>&1 || fail 8 unsupported "Remote feedback requires python3."
    exec python3 - "$json" "$@" <<'PY'
import json
import os
import secrets
import selectors
import shutil
import signal
import subprocess
import sys
import time

as_json = sys.argv[1] == "1"
args = sys.argv[2:]
wait = 0
if args:
    if len(args) != 2 or args[0] != "--wait" or not args[1].isascii() or not args[1].isdigit():
        print("Usage: sheprd browser feedback [--wait SECONDS]", file=sys.stderr)
        sys.exit(2)
    wait = int(args[1])
    if wait > 600:
        print("Wait must be between 0 and 600 seconds", file=sys.stderr)
        sys.exit(2)
core = shutil.which("herdr-core") or os.path.expanduser("~/.local/bin/herdr-core")
terminal = os.environ["HC_TERMINAL_ID"]
key = "herdr-gpui-" + secrets.token_hex(6)
reply = "sheprd-reply-" + key[len("herdr-gpui-"):]
deadline = time.monotonic() + wait + 30

def call(*arguments, cleanup=False):
    # Bound both memory and time while reading, without an unbounded disk spool.
    command = [core, *arguments]
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
acknowledged = False
status = 1
try:
    call("label", terminal, key + "=feedback-" + str(wait))
    while time.monotonic() < deadline:
        record = json.loads(call("get", terminal, "--json"))
        labels = record["labels"]
        if reply in labels:
            result = json.loads(labels[reply])
            notes = result["notes"]
            if notes is not None and not isinstance(notes, str):
                raise ValueError("Invalid notes reply")
            print(json.dumps(result, ensure_ascii=False) if as_json else
                  (notes if notes is not None else "No notes from the user yet"), flush=True)
            call("label", terminal, key + "=ack")
            acknowledged = True
            status = 0 if notes is not None else 4
            break
        if key not in labels or record.get("state") == "finished":
            raise ValueError("Browser request was cancelled or its terminal ended")
        time.sleep(0.2)
    else:
        raise TimeoutError("No reply from the Sheprd window before the timeout")
except KeyboardInterrupt:
    status = 130
except (OSError, ValueError, KeyError, subprocess.SubprocessError, TimeoutError) as error:
    if as_json:
        print(json.dumps({"error": {"code": "unavailable", "message": str(error)}}))
    else:
        print(str(error), file=sys.stderr)
finally:
    if not acknowledged:
        try:
            call("label", terminal, key + "=", reply + "=", cleanup=True)
        except (OSError, subprocess.SubprocessError, ValueError):
            pass
sys.exit(status)
PY
}

# Options apply to every command, wherever they appear.
for arg in "$@"; do
    [ "$arg" = --json ] && json=1
done
# Work uses a separate namespace and the app's normal confirmation flows.
skip_window=0
family=
for arg in "$@"; do
    if [ "$skip_window" = 1 ]; then skip_window=0; continue; fi
    case $arg in
        --json | --window=*) continue ;;
        --window) skip_window=1; continue ;;
        *) family=$arg; break ;;
    esac
done
case $family in
    notes | worktree | scripts | checkpoint | teleport | fan-out)
        core_terminal || unavailable
        command -v python3 >/dev/null 2>&1 || fail 8 unsupported "Remote work requires python3."
        core_work "$@"
        exit $?
        ;;
esac
for arg in "$@"; do
    case $arg in
        --window | --window=*) unavailable ;;
    esac
done
for arg in "$@"; do
    shift
    [ "$arg" = --json ] || set -- "$@" "$arg"
done

case ${1:-} in
    "" | -h | --help | help) usage ;;
    feedback)
        shift
        core_feedback "$@"
        ;;
    browser)
        shift
        case ${1:-} in
            "" | -h | --help | help) usage ;;
            open)
                shift
                open_page "$@"
                ;;
            reload)
                shift
                reload_pages "$@"
                ;;
            feedback)
                shift
                core_feedback "$@"
                ;;
            *) unavailable ;;
        esac
        ;;
    *) unavailable ;;
esac
