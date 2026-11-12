# Runs px0's workbench on a host for Sheprd's px0 tabs while the app keeps
# this script's input open. The app reads the workbench's address from px0's
# own `workbench.lock`; this only finds px0 and ties its life to the input.
# Exit codes: 127 px0 not found, otherwise px0's own.
set -u
# Sheprd never installs px0: it uses the one on PATH, in the user's own bin
# folders where its installer puts it, or, since `ssh` gives no login shell,
# on the login shell's PATH.
px0=$(command -v px0 2>/dev/null) || px0=
for candidate in "$HOME/.local/bin/px0" "$HOME/bin/px0"; do
  if [ -z "$px0" ] && [ -x "$candidate" ]; then
    px0=$candidate
  fi
done
if [ -z "$px0" ] && [ -n "${SHELL:-}" ]; then
  px0=$("$SHELL" -lc 'command -v px0' </dev/null 2>/dev/null | tail -n 1)
fi
case $px0 in
  /*) [ -x "$px0" ] || exit 127 ;;
  *) exit 127 ;;
esac
# A registry of Sheprd's own, as on this computer, so a bare `px0` the user
# runs on the host is never raced; px0's settings still come from HOME.
XDG_STATE_HOME="$HOME/.herdr-gpui/px0-state" "$px0" -no-open -no-update \
  -no-telemetry -no-color -host 127.0.0.1 -port 0 -base-path / \
  </dev/null >/dev/null 2>&1 &
pid=$!
# The input ends when the app stops this workbench, quits, or crashes, and
# when the connection drops. px0 stops its per-checkout children itself when
# hung up. A background job reads /dev/null, so the input goes to it as fd 3.
exec 3<&0
{
  cat <&3 >/dev/null
  kill -HUP "$pid"
} </dev/null >/dev/null 2>&1 &
exec 3<&-
# This ends with px0, so the app sees its exit as the `ssh`'s.
wait "$pid"
