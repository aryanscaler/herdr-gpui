# Runs px0's workbench on a host for Sheprd's px0 tabs. Once the workbench
# listens, this prints `port N`, its loopback port on the host, which the app
# forwards to itself. Standard input stays open while the app wants the
# workbench; its end stops a workbench this started. A workbench already
# running from an earlier start is reused and left running.
# Exit codes: 2 setup, 3 px0 not found, 4 no address in time, 5 px0 stopped.
set -u
umask 077
# Sheprd never installs px0: it uses the one on PATH, in the user's own bin
# folders where its installer puts it, or, since `ssh` gives no login shell,
# on the login shell's PATH.
px0=$(command -v px0 2>/dev/null) || px0=
if [ -z "$px0" ]; then
  for candidate in "$HOME/.local/bin/px0" "$HOME/bin/px0"; do
    if [ -x "$candidate" ]; then
      px0=$candidate
      break
    fi
  done
fi
if [ -z "$px0" ] && [ -n "${SHELL:-}" ]; then
  px0=$("$SHELL" -lc 'command -v px0' </dev/null 2>/dev/null | tail -n 1)
fi
case $px0 in
  /*) [ -x "$px0" ] || exit 3 ;;
  *) exit 3 ;;
esac
# A registry of Sheprd's own, as on this computer, so a bare `px0` the user
# runs on the host is never raced; px0's settings still come from HOME.
state="$HOME/.herdr-gpui/px0-state"
lock="$state/px0/workbench.lock"
mkdir -p "$state" || exit 2
field() {
  sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\{0,1\}\([^\",]*\).*/\1/p" "$lock" 2>/dev/null | head -n 1
}
port_of() {
  case $(field url) in
    http://127.0.0.1:*) field url | sed 's|^http://127\.0\.0\.1:\([0-9][0-9]*\).*|\1|' ;;
  esac
}
owner=$(field pid)
port=$(port_of)
if [ -n "$owner" ] && [ -n "$port" ] && kill -0 "$owner" 2>/dev/null; then
  echo "port $port"
  # Not this start's to stop: wait for the app to go. Should the workbench
  # stop first, the app's own checks see it and offer a new start.
  cat >/dev/null
  exit 0
fi
rm -f "$lock"
XDG_STATE_HOME="$state" "$px0" -no-open -no-update -no-color -host 127.0.0.1 -port 0 \
  -base-path / </dev/null >/dev/null 2>&1 &
pid=$!
shell=$$
# px0 starts a child per checkout, and a host need not give it a process
# group of its own, so its whole tree is stopped.
tree() {
  for child in $(ps -A -o pid= -o ppid= | awk -v parent="$1" '$2 == parent { print $1 }'); do
    tree "$child"
  done
  echo "$1"
}
stop() {
  # shellcheck disable=SC2046 # one PID per word
  kill -TERM $(tree "$pid") 2>/dev/null
}
waited=0
port=
while [ -z "$port" ]; do
  kill -0 "$pid" 2>/dev/null || exit 5
  if [ "$(field pid)" = "$pid" ]; then
    port=$(port_of)
  fi
  if [ -z "$port" ]; then
    if [ "$waited" -ge 30 ]; then
      stop
      exit 4
    fi
    sleep 1
    waited=$((waited + 1))
  fi
done
echo "port $port"
# Some SSH servers kill this shell when the connection closes rather than
# end its stdin, so a watcher stops px0 once the shell is gone.
(
  trap '' HUP
  while kill -0 "$shell" 2>/dev/null; do sleep 2; done
  stop
) </dev/null >/dev/null 2>&1 &
shell_watch=$!
# The end of the input, as the app goes away, stops px0. A job started in
# the background reads /dev/null, so the input goes to it as fd 3.
exec 3<&0
(
  cat <&3 >/dev/null
  stop
) </dev/null >/dev/null 2>&1 &
input_watch=$!
exec 3<&-
# Either way this ends once px0 does, so the app sees the SSH end.
wait "$pid"
status=$?
stop
kill "$shell_watch" "$input_watch" 2>/dev/null
exit "$status"
