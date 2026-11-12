import { spawn } from "node:child_process";
import { isAbsolute } from "node:path";

// Dependencies are injectable so tests never touch a daemon or agent process.
export function setup(api, runtime = {
  spawn, process, setInterval, clearInterval, setTimeout, clearTimeout,
}) {
  const env = runtime.process.env;
  if (!env.HC_TERMINAL_ID || !env.HC_TERMINAL_SOCKET) return () => {};
  const override = env.SHEPRD_ACP_BINARY;
  const program = override && isAbsolute(override) ? override : "sheprd-acp";
  // Memory survives plugin reloads within this TUI; stale child completions must
  // never overwrite a newer selection, including a newer explicit clear.
  const [state, update] = api.storage.memory("sheprd-session-sequence", {
    initial: { sequence: 0 },
  });
  let previous;
  let pending;
  let active = false;
  let stopped = false;
  let retry;

  function later() {
    if (retry !== undefined) return;
    retry = runtime.setTimeout(() => { retry = undefined; pump(); }, 1000);
    retry.unref?.();
  }

  function pump() {
    if (active || pending === undefined) return;
    if (retry !== undefined) { runtime.clearTimeout(retry); retry = undefined; }
    if (pending.sequence < state.sequence) { pending = undefined; return; }
    const payload = pending;
    pending = undefined;
    active = true;
    let child;
    try {
      child = runtime.spawn(program, ["session-report", "opencode"], {
        env, stdio: ["pipe", "ignore", "ignore"], windowsHide: true,
      });
    } catch {
      active = false;
      pending ??= payload;
      later();
      return;
    }
    const timeout = runtime.setTimeout(() => child.kill("SIGKILL"), 1500);
    timeout.unref?.();
    let finished = false;
    function finish(code) {
      if (finished) return;
      finished = true;
      runtime.clearTimeout(timeout);
      active = false;
      if (code === 0) pump();
      else { pending ??= payload; later(); }
    }
    child.once("error", finish);
    child.once("close", finish);
    child.stdin.on("error", () => {});
    child.stdin.end(JSON.stringify(payload));
  }

  function report(sessionID) {
    if (previous === sessionID) return;
    previous = sessionID;
    update((draft) => { draft.sequence++; });
    pending = {
      pid: runtime.process.pid,
      session_id: sessionID,
      sequence: state.sequence,
    };
    pump();
  }

  function sample() {
    if (stopped) return;
    const route = api.ui.router.current();
    const id = route?.type === "session" ? route.sessionID : null;
    // The output remains well below the CLI frame bound even for a bad route.
    report(typeof id === "string" && id.length > 0 && id.length <= 1024 ? id : null);
    pump();
  }

  sample();
  const interval = runtime.setInterval(sample, 250);
  interval.unref?.();
  return () => {
    if (stopped) return;
    stopped = true;
    runtime.clearInterval(interval);
    report(null);
  };
}
