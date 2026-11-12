import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../extension.mjs", import.meta.url), "utf8");

for (const agent of ["pi", "omp"]) {
  const { default: setup } = await import(`data:text/javascript,${encodeURIComponent(source.replace("__AGENT__", agent))}`);
  test(`${agent} reports exact session paths, coalesces switches, and clears on shutdown`, () => {
    const callbacks = new Map();
    const children = [];
    const runtime = {
      process: { pid: 42, env: { HC_TERMINAL_ID: "t1", HC_TERMINAL_SOCKET: "/owning.sock" } }, globalThis: {},
      setTimeout: () => 1, clearTimeout: () => {},
      spawn(program, args) {
        const child = new EventEmitter();
        child.stdin = new EventEmitter();
        child.stdin.end = (text) => { child.payload = JSON.parse(text); };
        child.kill = () => {};
        child.args = args;
        children.push(child);
        return child;
      },
    };
    const api = { on: (name, callback) => callbacks.set(name, callback) };
    setup(api, runtime);
    const context = (path, hasUI = true) => ({ hasUI, sessionManager: { getSessionFile: () => path } });
    callbacks.get("session_start")({}, context("/sessions/child", false));
    assert.equal(children.length, 0);
    callbacks.get("session_start")({}, context("/sessions/first"));
    callbacks.get("session_switch")({}, context("/sessions/second"));
    callbacks.get("session_switch")({}, context("/sessions/third"));
    assert.equal(children.length, 1);
    assert.equal(children[0].payload.session_path, "/sessions/first");
    children[0].emit("close", 0);
    assert.equal(children[1].payload.session_path, "/sessions/third");
    assert.equal(children[1].payload.sequence, 3);
    children[1].emit("close", 0);
    setup(api, runtime);
    callbacks.get("session_shutdown")({}, context("/sessions/third"));
    assert.equal(children[2].payload.session_path, null);
    assert.equal(children[2].payload.sequence, 4);
    assert.deepEqual(children[2].args, ["session-report", agent]);
  });

  test(`${agent} drops an old reporter's failed report once a reload supersedes it`, () => {
    const first = new Map();
    const second = new Map();
    const children = [];
    const timers = new Set();
    const runtime = {
      process: { pid: 42, env: { HC_TERMINAL_ID: "t1", HC_TERMINAL_SOCKET: "/owning.sock" } }, globalThis: {},
      setTimeout(fn) {
        const timer = () => { timers.delete(timer); fn(); };
        timers.add(timer);
        return timer;
      },
      clearTimeout(timer) { timers.delete(timer); },
      spawn() {
        const child = new EventEmitter();
        child.stdin = new EventEmitter();
        child.stdin.end = (text) => { child.payload = JSON.parse(text); };
        child.kill = () => {};
        children.push(child);
        return child;
      },
    };
    const context = (path) => ({ hasUI: true, sessionManager: { getSessionFile: () => path } });
    setup({ on: (name, callback) => first.set(name, callback) }, runtime);
    setup({ on: (name, callback) => second.set(name, callback) }, runtime);
    first.get("session_start")({}, context("/sessions/old"));
    second.get("session_start")({}, context("/sessions/new"));
    assert.equal(children.length, 2);
    children[1].emit("close", 0);
    children[0].emit("close", 1);
    for (const timer of [...timers]) timer();
    assert.equal(children.length, 2);
    assert.equal(timers.size, 0);
  });
}
