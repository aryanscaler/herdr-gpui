import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import test from "node:test";
import { setup } from "../opencode/reporter.mjs";

function fixture(env = { HC_TERMINAL_ID: "terminal", HC_TERMINAL_SOCKET: "/owning.sock" }) {
  let route = { type: "home" };
  let poll;
  const children = [];
  const deadlines = new Set();
  const state = { sequence: 0 };
  const api = {
    storage: { memory: () => [state, (fn) => fn(state)] },
    ui: { router: { current: () => route } },
  };
  const runtime = {
    process: { pid: 1234, env },
    setInterval(fn) { poll = fn; return fn; },
    clearInterval() { poll = undefined; },
    setTimeout(fn) {
      const timer = () => { deadlines.delete(timer); fn(); };
      deadlines.add(timer);
      return timer;
    },
    clearTimeout(fn) { deadlines.delete(fn); },
    spawn(program, args, options) {
      const child = new EventEmitter();
      const emit = child.emit;
      child.emit = (event, ...args) => emit.call(child, event, ...(event === "close" && args.length === 0 ? [0] : args));
      child.stdin = new EventEmitter();
      child.stdin.end = (payload) => { child.payload = JSON.parse(payload); };
      child.kill = () => { child.killed = true; };
      Object.assign(child, { program, args, options });
      children.push(child);
      return child;
    },
  };
  return {
    api, runtime, children, deadlines,
    start: () => setup(api, runtime),
    select(value) { route = value; poll?.(); },
    tick() { poll?.(); },
  };
}

test("reports explicit selection and none, with TUI pid and increasing sequence", () => {
  const f = fixture();
  const stop = f.start();
  assert.deepEqual(f.children[0].payload, { pid: 1234, session_id: null, sequence: 1 });
  f.children[0].emit("close");
  f.select({ type: "session", sessionID: "session-a" });
  assert.deepEqual(f.children[1].payload, { pid: 1234, session_id: "session-a", sequence: 2 });
  f.children[1].emit("close");
  f.tick();
  assert.equal(f.children.length, 2);
  f.select({ type: "plugin", sessionID: "not-selected" });
  assert.equal(f.children[2].payload.session_id, null);
  f.children[2].emit("close");
  stop();
  assert.equal(f.deadlines.size, 0);
});

test("coalesces rapid navigation and cleanup clears after an outstanding report", () => {
  const f = fixture();
  const stop = f.start();
  f.select({ type: "session", sessionID: "a" });
  f.select({ type: "session", sessionID: "b" });
  assert.equal(f.children.length, 1);
  f.children[0].emit("close");
  assert.deepEqual(f.children[1].payload, { pid: 1234, session_id: "b", sequence: 3 });
  stop();
  f.select({ type: "session", sessionID: "ignored" });
  f.children[1].emit("close");
  assert.deepEqual(f.children[2].payload, { pid: 1234, session_id: null, sequence: 4 });
  f.children[2].emit("close");
  stop();
  assert.equal(f.children.length, 3);
});

test("sequence survives reload and oversized or empty selections clear", () => {
  const f = fixture();
  const stop = f.start();
  f.children[0].emit("close");
  stop();
  const stopAgain = f.start();
  assert.equal(f.children[1].payload.sequence, 2);
  f.children[1].emit("close");
  f.select({ type: "session", sessionID: "valid" });
  f.children[2].emit("close");
  f.select({ type: "session", sessionID: "x".repeat(1025) });
  assert.equal(f.children[3].payload.session_id, null);
  f.children[3].emit("close");
  f.select({ type: "session", sessionID: "" });
  assert.equal(f.children.length, 4);
  stopAgain();
});

test("requires core routing and honors only an absolute managed override", () => {
  const inactive = fixture({ HERDR_PANE_ID: "legacy" });
  inactive.start()();
  assert.equal(inactive.children.length, 0);
  for (const [override, expected] of [
    [undefined, "sheprd-acp"], ["relative", "sheprd-acp"],
    ["/managed path/sheprd-acp", "/managed path/sheprd-acp"],
  ]) {
    const f = fixture({ HC_TERMINAL_ID: "core", HC_TERMINAL_SOCKET: "/owning.sock", SHEPRD_ACP_BINARY: override });
    const stop = f.start();
    assert.equal(f.children[0].program, expected);
    assert.deepEqual(f.children[0].args, ["session-report", "opencode"]);
    assert.deepEqual(f.children[0].options.stdio, ["pipe", "ignore", "ignore"]);
    assert.equal(f.children[0].options.shell, undefined);
    f.children[0].emit("close");
    stop();
  }
});

test("deadline kills only the reporter and error/close finish at most once", () => {
  const f = fixture();
  const stop = f.start();
  f.select({ type: "session", sessionID: "next" });
  [...f.deadlines][0]();
  assert.equal(f.children[0].killed, true);
  assert.equal(f.children.length, 1);
  f.children[0].stdin.emit("error", new Error("broken pipe"));
  f.children[0].emit("error", new Error("missing binary"));
  f.children[0].emit("close");
  f.tick();
  assert.equal(f.children.length, 2);
  f.children[1].emit("close");
  stop();
  f.children[2].emit("close");
  assert.equal(f.deadlines.size, 0);
});

test("synchronous spawn failure is nonfatal", () => {
  const f = fixture();
  f.runtime.spawn = () => { throw new Error("unavailable"); };
  const stop = f.start();
  assert.doesNotThrow(() => f.select({ type: "session", sessionID: "a" }));
  assert.doesNotThrow(stop);
});

test("failed selection and clear are retried without reverting to an older route", () => {
  const f = fixture();
  const stop = f.start();
  f.children[0].emit("close");
  f.select({ type: "session", sessionID: "a" });
  f.children[1].emit("close");
  f.select({ type: "session", sessionID: "b" });
  f.children[2].emit("close", 1);
  f.tick();
  assert.deepEqual(f.children[3].payload, f.children[2].payload);
  f.children[3].emit("close");
  stop();
  f.children[4].emit("close", 1);
  [...f.deadlines][0]();
  assert.equal(f.children[5].payload.session_id, null);
  f.children[5].emit("close");
});

test("a replacement plugin supersedes an old failed cleanup without endless helpers", () => {
  const f = fixture();
  const stopOld = f.start();
  f.children[0].emit("close");
  f.select({ type: "session", sessionID: "a" });
  stopOld();
  const stopNew = f.start();
  assert.equal(f.children.length, 3);
  f.children[1].emit("close", 1);
  f.children[2].emit("close");
  [...f.deadlines][0]();
  assert.equal(f.children.length, 3);
  assert.equal(f.deadlines.size, 0);
  stopNew();
  f.children[3].emit("close");
});
