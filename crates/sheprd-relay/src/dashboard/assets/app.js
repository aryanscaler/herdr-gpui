// The relay dashboard: draws the snapshots /dashboard/api/live streams each
// second (Server-Sent Events), falling back to polling /dashboard/api/stats
// when a stream cannot open. Read-only; every value goes in through
// textContent, never as HTML.
"use strict";

const POLL_MS = 5000;
let lastTotals = null; // { t, bytes } for the live throughput tile
const SAMPLES = 120; // ten minutes of the relay's five-second samples
let samples = []; // { t, up, down } in bytes per second, from the relay

const $ = (id) => document.getElementById(id);

function bytes(value) {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 && unit > 0 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

function duration(seconds) {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
  return `${Math.floor(seconds / 86400)}d ${Math.floor((seconds % 86400) / 3600)}h`;
}

function count(value) {
  return value.toLocaleString();
}

function limit(value, unit) {
  return value > 0 ? `of ${unit ? bytes(value) + "/s" : count(value)}` : "no limit";
}

function setText(id, text) {
  $(id).textContent = text;
}

function rows(body, entries) {
  const table = $(body);
  table.replaceChildren(
    ...entries.map((cells) => {
      const row = document.createElement("tr");
      cells.forEach((cell, index) => {
        const td = document.createElement("td");
        td.textContent = cell;
        const text = (body === "active" && index === 1) || (body === "bans" && index === 1);
        if (index > 0 && !text) td.className = "num";
        row.append(td);
      });
      return row;
    }),
  );
}

function label(text) {
  return text.charAt(0).toUpperCase() + text.slice(1).replaceAll("_", " ");
}

function render(stats) {
  const totals = stats.totals;
  const limits = stats.limits;
  setText("version", stats.version);
  setText("sessions", count(totals.sessions));
  setText("sessions-limit", `${limit(limits.max_sessions)} · ${count(totals.pending)} waiting`);
  setText("listeners", count(totals.listeners));
  setText("listeners-limit", limit(limits.max_listeners));
  setText("connections", count(totals.connections));
  setText("connections-limit", limit(limits.max_connections));
  $("connections-limit").title = `${count(limits.max_connections_per_ip)} per address`;
  setText("carried", bytes(totals.bytes_from_dialers + totals.bytes_from_listeners));
  setText("uptime", `up ${duration(totals.uptime_secs)}`);
  setText("refused", count(totals.refused));
  setText(
    "bandwidth-limit",
    limits.session_bandwidth > 0 ? `${bytes(limits.session_bandwidth)}/s a session` : "no session limit",
  );
  $("bandwidth-limit").title =
    limits.total_bandwidth > 0 ? `${bytes(limits.total_bandwidth)}/s in total` : "No total limit";

  samples = stats.history
    .slice(-SAMPLES)
    .map((sample) => ({ t: sample.at_ms, up: sample.from_dialers, down: sample.from_listeners }));
  // The tile shows this second's rate, from the live totals; the chart
  // keeps the relay's five-second history.
  const carried = totals.bytes_from_dialers + totals.bytes_from_listeners;
  const now = Date.now();
  if (lastTotals && now > lastTotals.t) {
    const rate = Math.max(0, carried - lastTotals.bytes) / ((now - lastTotals.t) / 1000);
    setText("rate", `${bytes(rate)}/s`);
  } else {
    const last = samples[samples.length - 1];
    setText("rate", last ? `${bytes(last.up + last.down)}/s` : "–");
  }
  lastTotals = { t: now, bytes: carried };

  rows("dials", totals.dials.map(([name, value]) => [label(name), count(value)]));
  rows("listens", totals.listens.map(([name, value]) => [label(name), count(value)]));
  rows(
    "active",
    stats.sessions.map((row) => [
      row.route,
      row.pairing ? "Pairing" : "Session",
      duration(row.seconds),
      bytes(row.bytes_from_dialer),
      bytes(row.bytes_from_listener),
    ]),
  );
  if (stats.sessions.length === 0) rows("active", [["No sessions", "", "", "", ""]]);
  rows(
    "bans",
    stats.bans.map((ban) => [
      ban.address,
      label(ban.reason),
      ban.manual ? "When removed" : duration(ban.seconds_left),
    ]),
  );
  if (stats.bans.length === 0) rows("bans", [["No bans", "", ""]]);
  draw();
  showHover();
}

const SVG = "http://www.w3.org/2000/svg";
const PAD = { left: 64, right: 12, top: 12, bottom: 20 };

function node(name, attributes) {
  const element = document.createElementNS(SVG, name);
  for (const [key, value] of Object.entries(attributes)) element.setAttribute(key, value);
  return element;
}

function niceMax(value) {
  if (value <= 0) return 1024;
  const step = 2 ** Math.ceil(Math.log2(value));
  return step;
}

function geometry() {
  const svg = $("chart");
  const width = svg.clientWidth;
  const height = svg.clientHeight;
  const top = niceMax(Math.max(0, ...samples.map((s) => Math.max(s.up, s.down))));
  const span = SAMPLES - 1;
  const x = (index) => PAD.left + ((width - PAD.left - PAD.right) * (index + SAMPLES - samples.length)) / span;
  const y = (value) => PAD.top + (height - PAD.top - PAD.bottom) * (1 - value / top);
  return { svg, width, height, top, x, y };
}

function draw() {
  const { svg, width, height, top, x, y } = geometry();
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  const children = [];
  for (const fraction of [0, 0.5, 1]) {
    const value = top * fraction;
    children.push(node("line", { class: "grid", x1: PAD.left, x2: width - PAD.right, y1: y(value), y2: y(value) }));
    const text = node("text", { class: "axis", x: PAD.left - 8, y: y(value) + 4, "text-anchor": "end" });
    text.textContent = `${bytes(value)}/s`;
    children.push(text);
  }
  for (const [key, cls] of [["up", "s1"], ["down", "s2"]]) {
    if (samples.length < 2) break;
    const points = samples.map((sample, index) => `${x(index).toFixed(1)},${y(sample[key]).toFixed(1)}`);
    children.push(node("polyline", { class: `line ${cls}`, points: points.join(" ") }));
  }
  svg.replaceChildren(...children);
}

let pointerX = null; // client x of the pointer over the chart, kept across refreshes

function hover(event) {
  pointerX = event.clientX;
  showHover();
}

function showHover() {
  const tooltip = $("tooltip");
  if (samples.length === 0 || pointerX === null) return;
  const { svg, x, y, height } = geometry();
  const box = svg.getBoundingClientRect();
  const pointer = pointerX - box.left;
  let nearest = 0;
  samples.forEach((_, index) => {
    if (Math.abs(x(index) - pointer) < Math.abs(x(nearest) - pointer)) nearest = index;
  });
  const sample = samples[nearest];
  draw();
  svg.append(node("line", { class: "cross", x1: x(nearest), x2: x(nearest), y1: PAD.top, y2: height - PAD.bottom }));
  svg.append(node("circle", { class: "dot s1", cx: x(nearest), cy: y(sample.up), r: 4 }));
  svg.append(node("circle", { class: "dot s2", cx: x(nearest), cy: y(sample.down), r: 4 }));
  const time = new Date(sample.t).toLocaleTimeString();
  tooltip.textContent = `${time} · from dialers ${bytes(sample.up)}/s · from listeners ${bytes(sample.down)}/s`;
  tooltip.hidden = false;
  const left = Math.min(Math.max(x(nearest) + 12, 0), box.width - tooltip.offsetWidth - 8);
  tooltip.style.left = `${left}px`;
}

function leave() {
  pointerX = null;
  $("tooltip").hidden = true;
  draw();
}

async function poll() {
  try {
    const response = await fetch("/dashboard/api/stats", { credentials: "same-origin", cache: "no-store" });
    if (response.status === 401) {
      window.location.assign("/dashboard/login");
      return;
    }
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    render(await response.json());
    setText("status", `Updated ${new Date().toLocaleTimeString()}`);
  } catch (error) {
    setText("status", `Could not refresh: ${error.message}`);
  }
}

let polling = null;

// Polls instead, while the live stream cannot open (for example when the
// relay already serves as many streams as it allows).
function startPolling() {
  if (polling) return;
  poll();
  polling = setInterval(poll, POLL_MS);
}

function live() {
  const source = new EventSource("/dashboard/api/live");
  source.addEventListener("stats", (event) => {
    if (polling) {
      clearInterval(polling);
      polling = null;
    }
    render(JSON.parse(event.data));
    setText("status", "Live");
  });
  source.addEventListener("error", async () => {
    // EventSource reconnects on its own after a drop; a refused stream
    // (signed out, or too many open) closes it instead.
    if (source.readyState !== EventSource.CLOSED) {
      setText("status", "Reconnecting…");
      return;
    }
    // Whatever the check finds, even a network error, keep the page
    // recovering: poll, and try the live stream again later.
    try {
      const check = await fetch("/dashboard/api/stats", { credentials: "same-origin", cache: "no-store" });
      if (check.status === 401) {
        window.location.assign("/dashboard/login");
        return;
      }
      setText("status", "Live updates unavailable; refreshing every 5 s");
    } catch (error) {
      setText("status", `Could not reach the relay: ${error.message}; retrying`);
    }
    startPolling();
    setTimeout(live, 30000);
  });
}

document.addEventListener("DOMContentLoaded", () => {
  const svg = $("chart");
  svg.addEventListener("pointermove", hover);
  svg.addEventListener("pointerleave", leave);
  window.addEventListener("resize", draw);
  live();
});
