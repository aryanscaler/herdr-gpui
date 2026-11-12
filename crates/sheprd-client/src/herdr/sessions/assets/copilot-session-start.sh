#!/bin/sh
# Installed only as a Copilot SessionStart hook. The reporter bounds and
# validates stdin (session_id or sessionId) and derives parent identity/scope.
[ -n "${HC_TERMINAL_ID:-}" ] || exit 0
[ -n "${HC_TERMINAL_SOCKET:-}" ] || exit 0
case "${SHEPRD_ACP_BINARY:-}" in
    /*) reporter=$SHEPRD_ACP_BINARY ;;
    *) reporter=sheprd-acp ;;
esac
exec "$reporter" session-report copilot
