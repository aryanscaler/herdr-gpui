#!/bin/sh
# Sheprd's independent SessionStart reporter; no Herdr 1 socket is used.
[ -n "${HC_TERMINAL_ID:-}" ] || exit 0
[ -n "${HC_TERMINAL_SOCKET:-}" ] || exit 0
case "${SHEPRD_ACP_BINARY:-}" in
  /*) program=$SHEPRD_ACP_BINARY ;;
  *) program=sheprd-acp ;;
esac
exec "$program" session-report claude
