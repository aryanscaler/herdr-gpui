#!/bin/sh
# Installed only as a Codex SessionStart hook. The reporter bounds and validates
# stdin (session_id and transcript_path), excludes CODEX_THREAD_ID mismatches,
# and derives the parent process identity and HC_TERMINAL_ID scope itself.
[ -n "${HC_TERMINAL_ID:-}" ] || exit 0
[ -n "${HC_TERMINAL_SOCKET:-}" ] || exit 0
case "${SHEPRD_ACP_BINARY:-}" in
    /*) reporter=$SHEPRD_ACP_BINARY ;;
    *) reporter=sheprd-acp ;;
esac
exec "$reporter" session-report codex
