import { spawn } from "node:child_process";
import { isAbsolute } from "node:path";

// Installer specializes these two constants for each separately named extension.
const AGENT = "__AGENT__";
const PROGRAM = "sheprd-acp";

export default function setup(api, runtime = { process, spawn, globalThis, setTimeout, clearTimeout }) {
  if (!runtime.process.env.HC_TERMINAL_ID || !runtime.process.env.HC_TERMINAL_SOCKET) return;
  const key = Symbol.for(`sheprd.session.${AGENT}`);
  const state = runtime.globalThis[key] ??= { sequence: 0 };
  let pending;
  let active = false;
  let retry;
  function later() {
    if (retry !== undefined) return;
    retry = runtime.setTimeout(() => { retry = undefined; pump(); }, 1000);
    retry.unref?.();
  }
  function pump() {
    if (active || !pending) return;
    if (retry !== undefined) { runtime.clearTimeout(retry); retry = undefined; }
    if (pending.sequence < state.sequence) { pending = undefined; return; }
    const payload = pending;
    pending = undefined;
    active = true;
    let child;
    try {
      child = runtime.spawn(PROGRAM, ["session-report", AGENT], {
        env: runtime.process.env, stdio: ["pipe", "ignore", "ignore"], windowsHide: true,
      });
    } catch { active = false; pending ??= payload; later(); return; }
    const timeout = runtime.setTimeout(() => child.kill("SIGKILL"), 2000);
    timeout.unref?.();
    let finished = false;
    const finish = (code) => {
      if (finished) return;
      finished = true;
      runtime.clearTimeout(timeout);
      active = false;
      if (code === 0) pump();
      else {
        pending ??= payload;
        later();
      }
    };
    child.once("error", finish);
    child.once("close", finish);
    child.stdin.on("error", () => {});
    child.stdin.end(JSON.stringify(payload));
  }
  function report(context, clear = false) {
    if (!context?.hasUI) return;
    let path;
    try { path = context.sessionManager.getSessionFile(); } catch {}
    pending = {
      pid: runtime.process.pid,
      sequence: ++state.sequence,
      session_path: !clear && typeof path === "string" && isAbsolute(path) && path.length <= 4096 ? path : null,
    };
    pump();
  }
  api.on("session_start", (_event, context) => report(context));
  api.on("session_switch", (_event, context) => report(context));
  api.on("session_shutdown", (_event, context) => report(context, true));
}
