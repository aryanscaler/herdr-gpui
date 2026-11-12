#!/bin/sh
# Separately installed Sheprd SessionStart integration. Letta injects hook stdout
# into the next message, so successful and failed reports remain silent.
[ -n "${HC_TERMINAL_ID:-}" ] || exit 0
[ -n "${HC_TERMINAL_SOCKET:-}" ] || exit 0
case "${SHEPRD_ACP_BINARY:-}" in
  /*) program=$SHEPRD_ACP_BINARY ;;
  *) program=sheprd-acp ;;
esac
exec "$program" session-report letta >/dev/null 2>&1
