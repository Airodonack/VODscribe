// Switching between library entries, timed and watched for jank: from a
// click on a VOD's row to its row showing selected, its player's stage
// sized, its transcript's first line, its chat's first message and its
// chapters and highlights drawn, and to the page settled (no change to the
// page for 500 ms); and, for three seconds after the click, every frame's
// place and size of the window's parts (the library, the edges, the
// player, the transcript's head and first line, the pane), each move of
// more than 2 px logged with when it came. WebKit has no layout-shift API,
// so the page measures itself, from one script that clicks and resolves when
// it has settled (lib/app.mjs `js`), with the stretches the page was busy.
//
// The library is long VODs as people keep them: a transcript of thousands
// of lines with chapters, highlights and a cast, and thousands of chat
// messages, imported from an annotations file (import_annotations_file),
// three times over under ids of their own; and small ones, one with chapters
// and one without. The big file isn't in the repository: E2E_SWITCH_FIXTURE
// names it (an annotations file, v1, of a Twitch VOD, with its .chat.json
// beside it); without it, this skips.
//
// Each switch is measured at each width in E2E_SWITCH_WIDTHS (default 1280
// and 1920 CSS pixels wide, the library showing): heavy to heavy, heavy to
// light and back, and to and from the VOD without chapters; each step is the
// median of its switches (logged, with no budget). Then a few switches are
// filmed (ffmpeg's x11grab, ten frames a second, the first 2.5 s) into
// E2E_OUT/perf-switch/, with the jumps of each in jank-*.json.

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { resizeInner } from "../lib/app.mjs";
import { Measures, median } from "../lib/budgets.mjs";
import { windowRect } from "../lib/os.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const MINUTE_FIXTURE = path.join(here, "..", "fixtures", "annotations-minute.json");

/** How many times each kind of switch is measured. */
const TIMES = 5;
/** How long each switch is watched, at most (ms); jumps are logged for the first JANK_MS. */
const WATCH_MS = 6000;
const JANK_MS = 3000;

// The parts of the window whose place and size are watched, as CSS selectors:
// the first match of each, in the new VOD's view where there's an old one.
const PARTS = {
  library: ".library-pane",
  "left edge": ".edge-strip.left",
  "right edge": ".edge-strip.right",
  toolbar: ".main > .toolbar",
  "player panel": ".main > section",
  "player head": ".main > section > .head",
  stage: ".main .stage",
  frame: ".main .stage .frame",
  "caption strip": ".main .caption-strip",
  timeline: ".main .timeline",
  "player bar": ".main .bar",
  "clip bar": ".main .clip-bar",
  "pane tabs": ".pane-tabs",
  "transcript body": ".below > .body",
  "transcript head": ".layout > .head",
  "cast button": ".layout > .head .cast-toggle",
  "chat head": ".layout > .head .chat-head",
  "first line": "#transcript .line",
  "first line text": "#transcript .line .row:not(.gap)",
  "first gap row": "#transcript .line .row.gap",
  "first chat": "#transcript .chat .message",
  minimap: ".rail",
  "pane column": ".pane.side",
  "pane list": ".annotations-pane .lists",
};

// The page's side: clicks VOD `key`'s row and watches until the page holds
// still (or WATCH_MS). `paneText`: a word the new VOD's pane shows (null: it
// has none). Returns each step's time from the click (ms; null if never),
// the jumps, and when the page was busy.
const SWITCH = `
const [key, paneText, PARTS, WATCH_MS, JANK_MS] = arguments;
const app = document.querySelector(".app");
// What's on screen now is the old VOD's: marked, so the new one's is told apart.
for (const el of document.querySelectorAll("#transcript, .main .stage, .layout > .head")) el.dataset.old = "1";
const fresh = (sel) => {
  for (const el of document.querySelectorAll(sel)) if (!el.closest("[data-old]")) return el;
  return null;
};
const rectOf = (el) => {
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) };
};
const look = () => {
  const out = {};
  for (const [name, sel] of Object.entries(PARTS)) {
    const el = fresh(sel);
    const r = rectOf(el);
    // Hidden (display: none) counts as gone.
    out[name] = r && (r.w > 0 || r.h > 0) ? r : null;
  }
  return out;
};
// The click's moment: what's timed is from here (set again just before the click).
let t0 = performance.now();
let lastChange = 0;
let changes = 0;
let lastTargets = [];
const describe = (n) => {
  const el = n.nodeType === 1 ? n : n.parentElement;
  if (!el) return "?";
  return el.tagName.toLowerCase() + (el.id ? "#" + el.id : "") + (el.className && typeof el.className === "string" ? "." + el.className.trim().split(/\\s+/).join(".") : "");
};
const observer = new MutationObserver((list) => {
  lastChange = performance.now() - t0;
  changes += list.length;
  lastTargets = list.slice(-3).map((m) => describe(m.target));
});
observer.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
const steps = { selected: null, stage: null, line: null, gap: null, chat: null, pane: null, paneGoneFor: 0 };
let before = look();
const jumps = [];
const frames = [];
let paneGoneAt = null;
const button = document.querySelector('.vod[data-key="' + key + '"] button.main');
if (!button) throw new Error("no row for " + key);
t0 = performance.now();
lastChange = 0;
button.click();
// The click's own handlers, run before it returns; the rest runs as tasks
// after it. A message posted to itself, over and over, finds when the page
// was free between them: the gaps are its busy stretches.
const clickMs = performance.now() - t0;
const free = [];
const beat = new MessageChannel();
beat.port1.onmessage = () => {
  const t = performance.now() - t0;
  free.push(t);
  if (t < 600) beat.port2.postMessage(0);
};
beat.port2.postMessage(0);
const sawPane = () => {
  const pane = document.querySelector("#annotations-panel");
  return !!pane && pane.getBoundingClientRect().width > 0 && (paneText === null || pane.textContent.includes(paneText));
};
await new Promise((done) => {
  const frame = (now) => {
    const t = performance.now() - t0;
    frames.push(Math.round(t));
    if (steps.selected === null && document.querySelector('.vod.selected[data-key="' + key + '"]')) steps.selected = t;
    const stage = fresh(".main .stage");
    if (steps.stage === null && stage && stage.getBoundingClientRect().height > 0) steps.stage = t;
    if (steps.line === null && fresh("#transcript .line .row:not(.gap)")) steps.line = t;
    // A part "not transcribed" shown for a VOD that's all transcribed: the
    // view before its lines are read.
    if (steps.gap === null && fresh("#transcript .line .row.gap")) steps.gap = t;
    if (steps.chat === null && fresh("#transcript .chat .message")) steps.chat = t;
    const paneThere = !!document.querySelector("#annotations-panel");
    if (!paneThere && paneGoneAt === null) paneGoneAt = t;
    if (paneThere && paneGoneAt !== null) {
      steps.paneGoneFor += t - paneGoneAt;
      paneGoneAt = null;
    }
    if (steps.pane === null && paneText !== null && sawPane()) steps.pane = t;
    if (t <= JANK_MS) {
      const seen = look();
      for (const name of Object.keys(PARTS)) {
        const a = before[name];
        const b = seen[name];
        if (!a && !b) continue;
        if (!a || !b) {
          jumps.push({ t: Math.round(t), part: name, what: a ? "gone" : "came", from: a, to: b });
          continue;
        }
        const d = Math.max(Math.abs(a.x - b.x), Math.abs(a.y - b.y), Math.abs(a.w - b.w), Math.abs(a.h - b.h));
        if (d > 2) jumps.push({ t: Math.round(t), part: name, what: "moved", from: a, to: b });
      }
      before = seen;
    }
    const quiet = performance.now() - t0 - lastChange;
    if ((t > JANK_MS && quiet >= 500) || t > WATCH_MS) return done();
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
});
observer.disconnect();
if (paneGoneAt !== null) steps.paneGoneFor += performance.now() - t0 - paneGoneAt;
const settledQuiet = performance.now() - t0 - lastChange >= 500;
// The longest gaps between frames: the page busy.
const gaps = frames.slice(1).map((f, i) => f - frames[i]);
return {
  steps,
  settled: settledQuiet ? lastChange : null,
  lastTargets,
  changes,
  jumps,
  longFrames: gaps.filter((g) => g > 50).length,
  worstFrame: Math.max(0, ...gaps),
  clickMs: Math.round(clickMs),
  // Stretches over 8 ms when the page couldn't take a message: [from, to].
  busy: free.slice(1).flatMap((t, i) => (t - free[i] > 8 ? [[Math.round(free[i]), Math.round(t)]] : [])),
  firstFree: free.length ? Math.round(free[0]) : null,
  nodes: {
    all: document.getElementsByTagName("*").length,
    lines: document.querySelectorAll("#transcript .line").length,
    messages: document.querySelectorAll("#transcript .message").length,
    pane: document.querySelectorAll("#annotations-panel *").length,
    library: document.querySelectorAll(".library-pane *").length,
  },
};
`;

// The page's side: clicks the heading's switch to `to` ("annotations" or
// "transcript") and returns the time (ms) to the first frame showing it: the
// pane with a word of the VOD's marks, or the transcript's rows; null if not
// within WATCH_MS.
const TAB = `
const [to, paneText, WATCH_MS] = arguments;
const shown = () => {
  if (to === "annotations") {
    const pane = document.querySelector("#annotations-panel");
    return !!pane && pane.getBoundingClientRect().height > 0 && pane.textContent.includes(paneText);
  }
  const row = document.querySelector("#transcript .line");
  return !!row && row.getBoundingClientRect().height > 0;
};
const button = document.querySelector("#tab-" + to);
if (!button) throw new Error("no switch to " + to);
const t0 = performance.now();
button.click();
return await new Promise((done) => {
  const frame = () => {
    const t = performance.now() - t0;
    if (shown()) return done(t);
    if (t > WATCH_MS) return done(null);
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
});
`;

/** An annotations file's copy for VOD `id`, its title and every chapter and highlight marked with `tag`. */
function copyOf(file, chat, id, tag, out, { bare = false, chatCount = null } = {}) {
  const doc = JSON.parse(readFileSync(file, "utf8"));
  doc.source.platform = "twitch";
  doc.source.id = id;
  doc.source.title = `${tag} ${doc.source.title}`;
  for (const m of [...(doc.chapters ?? []), ...(doc.highlights ?? [])]) m.title = `${tag} ${m.title}`;
  if (bare) {
    doc.chapters = [];
    doc.highlights = [];
    doc.cast = [];
  }
  const target = path.join(out, `${id}.json`);
  writeFileSync(target, JSON.stringify(doc));
  // The chat beside it: a VOD imported without one would fetch the site's.
  const messages = JSON.parse(readFileSync(chat, "utf8"));
  writeFileSync(path.join(out, `${id}.chat.json`), JSON.stringify(chatCount ? messages.slice(0, chatCount) : messages));
  return target;
}

const ms = (v) => (v === null ? null : Math.round(v));

export default {
  name: "perf-switch",
  async run(app, ctx) {
    const big = process.env.E2E_SWITCH_FIXTURE;
    if (!big || !existsSync(big)) ctx.skip("E2E_SWITCH_FIXTURE names no annotations file");
    const chat = big.replace(/\.json$/, ".chat.json");
    const out = path.join(ctx.out, "perf-switch");
    mkdirSync(out, { recursive: true });
    await app.toLibrary();

    // The library: three heavy VODs, a light one with chapters, one without.
    const made = [
      { name: "heavy A", id: "v2910708690", tag: "[A]", file: big },
      { name: "heavy B", id: "v2910708691", tag: "[B]", file: big },
      { name: "heavy C", id: "v2910708692", tag: "[C]", file: big },
      { name: "light", id: "v100000001", tag: "[L]", file: MINUTE_FIXTURE, chatCount: 40 },
      { name: "bare", id: "v100000002", tag: "[Z]", file: MINUTE_FIXTURE, chatCount: 40, bare: true },
    ];
    const vods = {};
    for (const v of made) {
      const file = copyOf(v.file, chat, v.id, v.tag, out, v);
      const from = ctx.clock();
      const { vod } = await app.invoke("import_annotations_file", { path: file });
      ctx.log(`imported ${v.name} (${v.id}) as VOD ${vod.id} in ${Math.round((ctx.clock() - from) * 1000)} ms`);
      vods[v.name] = { ...v, key: vod.id, paneText: v.bare ? null : `${v.tag} ` };
    }
    await sleep(2000);

    // What each command a switch calls costs on its own, on a heavy VOD.
    const heavy = vods["heavy A"].key;
    const ranges = (await app.invoke("transcript", { vodId: heavy })).slice(0, 60).map((l) => [l.start, l.end]);
    const commands = {
      list_vods: {},
      transcript: { vodId: heavy },
      transcript_stretches: { vodId: heavy },
      captions: { vodId: heavy },
      list_marks: { vodId: heavy },
      list_clips: { vodId: heavy },
      vod_annotations: { vodId: heavy },
      chat_overview: { vodId: heavy, duration: 30270, bins: 1000, events: true },
      chat_windows: { vodId: heavy, ranges, cap: 8 },
      chat_emotes: { vodId: heavy },
    };
    const costs = {};
    for (const [cmd, args] of Object.entries(commands)) {
      const samples = await app.js(
        `const [cmd, args] = arguments;
         const out = [];
         let size = 0;
         for (let i = 0; i < 5; i++) {
           const start = performance.now();
           const got = await window.__TAURI_INTERNALS__.invoke(cmd, args);
           out.push(performance.now() - start);
           size = JSON.stringify(got).length;
         }
         return { ms: out.map(Math.round), size };`,
        cmd,
        args,
      );
      costs[cmd] = { median: median(samples.ms), size: samples.size };
      ctx.log(`command ${cmd}: median ${median(samples.ms)} ms of ${samples.ms.join(", ")}, ${samples.size} bytes`);
    }

    const widths = (process.env.E2E_SWITCH_WIDTHS || "1280,1920").split(",").map(Number);
    const height = Number(process.env.E2E_SWITCH_HEIGHT || 1100);
    const measures = new Measures(ctx, "perf-switch");
    const report = { commands: costs, widths: {} };
    const switchTo = async (v) => {
      // A click on the row open would close it.
      if (await app.js(`return !!document.querySelector('.vod.selected[data-key="' + arguments[0] + '"]')`, v.key))
        return null;
      await sleep(400);
      return app.js(SWITCH, v.key, v.paneText, PARTS, WATCH_MS, JANK_MS);
    };
    for (const width of widths) {
      const got = await resizeInner(app, width, height);
      ctx.log(`window ${got.width}x${got.height}`);
      const kinds = {};
      const note = (kind, from, to, r) => {
        (kinds[kind] ??= []).push({ from: from.name, to: to.name, ...r });
        const s = r.steps;
        const label = `${kind} @${width}`;
        for (const [step, v] of [
          ["selected", s.selected],
          ["stage", s.stage],
          ["first line", s.line],
          ["first chat", s.chat],
          ["pane", s.pane],
          ["settled", r.settled],
        ])
          if (v !== null) measures.add(`${label}: ${step}`, ms(v), ctx.clock(), ctx.clock());
        ctx.log(
          `${label} ${from.name} → ${to.name}: selected ${ms(s.selected)}, stage ${ms(s.stage)}, line ${ms(s.line)}, ` +
            `gap row ${ms(s.gap)}, chat ${ms(s.chat)}, pane ${ms(s.pane)} (gone ${ms(s.paneGoneFor)}), settled ${ms(r.settled)}; ` +
            `${r.jumps.length} jumps, ${r.longFrames} frames over 50 ms (worst ${r.worstFrame}); ` +
            `nodes ${JSON.stringify(r.nodes)}; click ${r.clickMs}, busy ${JSON.stringify(r.busy)}`,
        );
      };
      const go = async (kind, from, to) => note(kind, from, to, await switchTo(to));
      const [A, B, C, L, Z] = ["heavy A", "heavy B", "heavy C", "light", "bare"].map((n) => vods[n]);
      // Each opened once first: the first visit reads what later ones have kept.
      await switchTo(Z);
      for (const v of [A, B, C, L]) await go("first visit", Z, v).then(() => switchTo(Z));
      const order = [A, B, C];
      for (let i = 0; i < TIMES + 1; i++) await go("heavy to heavy", order[i % 3], order[(i + 1) % 3]);
      await switchTo(A);
      for (let i = 0; i < TIMES; i++) {
        await go("heavy to light", A, L);
        await go("light to heavy", L, A);
      }
      for (let i = 0; i < 3; i++) {
        await go("heavy to bare", A, Z);
        await go("bare to heavy", Z, A);
      }
      report.widths[width] = { size: got, kinds };
    }
    // What the pane and the chat cost: heavy to heavy again at the widest
    // width (the pane a column of its own there, from 1440 px with the
    // library) with the pane's column hidden, then with chat hidden too.
    {
      const wide = Math.max(...widths);
      await resizeInner(app, wide, height);
      const [A, B, C] = ["heavy A", "heavy B", "heavy C"].map((n) => vods[n]);
      const kinds = {};
      const round = async (kind) => {
        await switchTo(A);
        for (const [from, to] of [
          [A, B],
          [B, C],
          [C, A],
          [A, B],
          [B, C],
        ]) {
          const r = await switchTo(to);
          (kinds[kind] ??= []).push({ from: from.name, to: to.name, ...r });
          const s = r.steps;
          for (const [step, v] of [
            ["first line", s.line],
            ["first chat", s.chat],
            ["settled", r.settled],
          ])
            if (v !== null) measures.add(`${kind} @${wide}: ${step}`, ms(v), ctx.clock(), ctx.clock());
          ctx.log(
            `${kind} ${from.name} → ${to.name}: line ${ms(s.line)}, chat ${ms(s.chat)}, settled ${ms(r.settled)}, click ${r.clickMs}, busy ${JSON.stringify(r.busy)}`,
          );
        }
      };
      await app.click(".edge-strip.right button");
      await round("pane hidden");
      await app.click(".layout > .head .chat-toggle");
      await round("pane and chat hidden");
      await app.click(".layout > .head .chat-toggle");
      await app.click(".edge-strip.right button");
      report.hidden = kinds;
    }
    // The transcript's heading's switch, on a window too narrow for the
    // pane's column (the first width, 1280 by default): from a click on a
    // segment to the frame that shows the other side, the pane's marks or
    // the transcript's rows, on a heavy VOD (95 marks).
    {
      await resizeInner(app, widths[0], height);
      const A = vods["heavy A"];
      await switchTo(A);
      await sleep(800);
      const tabs = { annotations: [], transcript: [] };
      if (await app.js(`return !!document.querySelector("#tab-annotations")`)) {
        for (let i = 0; i < TIMES; i++)
          for (const to of ["annotations", "transcript"]) {
            await sleep(400);
            const t = await app.js(TAB, to, A.paneText, WATCH_MS);
            tabs[to].push(t);
          }
        for (const [to, list] of Object.entries(tabs)) {
          ctx.log(`tab to ${to} @${widths[0]}: ${list.map(ms).join(", ")} ms`);
          for (const v of list)
            if (v !== null) measures.add(`tab to ${to} @${widths[0]}`, ms(v), ctx.clock(), ctx.clock());
        }
      } else ctx.log(`no transcript/pane switch at ${widths[0]} px`);
      report.tabs = tabs;
    }
    writeFileSync(path.join(out, "switches.json"), JSON.stringify(report, null, 2));

    // A few switches filmed, at the first width: ten frames a second for
    // 2.5 s from just before the click, beside the jumps logged in the page.
    if (process.platform === "linux" && process.env.DISPLAY) {
      await resizeInner(app, widths[0], height);
      const [A, B, L, Z] = ["heavy A", "heavy B", "light", "bare"].map((n) => vods[n]);
      await switchTo(A);
      for (const [n, to] of [
        ["1-heavy-to-heavy", B],
        ["2-heavy-to-heavy", A],
        ["3-heavy-to-light", L],
        ["4-light-to-heavy", A],
        ["5-heavy-to-bare", Z],
        ["6-bare-to-heavy", A],
      ]) {
        await sleep(600);
        const dir = path.join(out, `film-${n}`);
        mkdirSync(dir, { recursive: true });
        const win = await windowRect(app);
        const film = spawn(
          "ffmpeg",
          [
            ...["-loglevel", "error", "-y", "-f", "x11grab", "-framerate", "10", "-video_size"],
            `${win.w}x${win.h}`,
            ...["-i", `${process.env.DISPLAY}+${win.x},${win.y}`, "-t", "2.6", path.join(dir, "%02d.png")],
          ],
          { stdio: "inherit" },
        );
        await sleep(150);
        const r = await app.js(SWITCH, to.key, to.paneText, PARTS, WATCH_MS, JANK_MS);
        await new Promise((done) => (film.exitCode !== null ? done() : film.once("exit", done)));
        writeFileSync(path.join(dir, "jank.json"), JSON.stringify(r, null, 2));
        ctx.log(`filmed ${n} into ${dir}: ${r.jumps.length} jumps`);
      }
    }
    measures.check();
  },
};
