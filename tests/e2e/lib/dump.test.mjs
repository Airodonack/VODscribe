import assert from "node:assert/strict";
import { test } from "node:test";

import { appLine, formatDump } from "./dump.mjs";

const page = (over = {}) => ({
  errors: [],
  media: null,
  native: { state: { open: false, doc: null, embed: null, player: null } },
  timeShown: "0:01 / 0:05",
  visibility: "visible",
  focused: true,
  console: [],
  text: "Library",
  ...over,
});

const player = (over = {}) => ({
  state: { epoch: 3, playing: true, ready: true, ended: false, time: 1.25, speed: 1, sound_output: false },
  duration: 5.4,
  shown: 1.2,
  frame_pts: 1.2,
  frame_known: true,
  last_frame_ms_ago: 40,
  decoding: true,
  thread_alive: true,
  shown_count: 36,
  dropped: 0,
  ...over,
});

const line = (text, start) => text.split("\n").find((l) => l.startsWith(start));

test("the media element's state, with what it buffered", () => {
  const text = formatDump(
    page({
      media: {
        tag: "audio",
        src: "http://127.0.0.1:1/x.mp3",
        readyState: 1,
        networkState: 2,
        paused: false,
        seeking: true,
        ended: false,
        currentTime: 0.88,
        duration: 5.4,
        buffered: [
          [0, 0.9],
          [2, 3.5],
        ],
        error: null,
      },
    }),
    "app: pid 1, running",
  );
  assert.equal(
    line(text, "media element:"),
    "media element: <audio> playing, seeking at 0.880 s of 5.400 s; readyState 1, networkState 2; " +
      "buffered 0.000–0.900, 2.000–3.500; error none; src http://127.0.0.1:1/x.mp3",
  );
  assert.equal(line(formatDump(page(), ""), "media element:"), "media element: no <video> or <audio>");
});

test("the native player open, playing, with its decoder and thread", () => {
  const text = formatDump(
    page({ native: { state: { open: true, doc: { kind: "vod", id: 7 }, embed: null, player: player() } } }),
    "",
  );
  assert.equal(
    line(text, "native player:"),
    "native player: vod 7 open, playing at 1.250 s of 5.400 s, epoch 3, ready, speed 1, no sound output; " +
      "last frame pts 1.200 s (shown at 1.200 s, 40 ms ago); decoder running; thread alive; 36 frames shown, 0 dropped",
  );
});

test("a stalled native player says what's missing", () => {
  const stalled = player({
    state: { epoch: 4, playing: true, ready: false, ended: false, time: 1, speed: 1, sound_output: true },
    frame_pts: null,
    shown: null,
    last_frame_ms_ago: null,
    decoding: false,
    thread_alive: false,
  });
  const text = formatDump(
    page({ native: { state: { open: true, doc: { kind: "edit", id: 2 }, player: stalled } } }),
    "",
  );
  assert.match(line(text, "native player:"), /edit 2 open, playing at 1\.000 s .* waiting for its first frame/);
  assert.match(line(text, "native player:"), /no frame; decoder none; thread ended/);
  const locked = formatDump(
    page({ native: { state: { open: true, doc: null, player: player({ state: null, frame_known: false }) } } }),
    "",
  );
  assert.match(line(locked, "native player:"), /its clock's lock is held .*; the frame's lock is held/);
});

test("the native player closed, busy, missing from the build or not answering", () => {
  const native = (n) => line(formatDump(page({ native: n }), ""), "native player:");
  assert.equal(native({ state: { open: false } }), "native player: none open");
  assert.match(native({ state: { open: null } }), /its lock is held/);
  assert.equal(
    native({ error: "command e2e_player_state not found" }),
    "native player: not in this build (no e2e feature)",
  );
  assert.equal(native({ error: "no answer in 2000 ms" }), "native player: couldn't be asked: no answer in 2000 ms");
});

test("the page's focus, errors and text; without the text for a log", () => {
  const text = formatDump(page({ focused: false, errors: ["It can't be played"] }), "app: pid 1, running");
  assert.equal(text.split("\n")[0], "the page shows: It can't be played");
  assert.equal(line(text, "page:"), "page: visible, not focused");
  assert.equal(text.split("\n").at(-1), "Library");
  assert.ok(!formatDump(page(), "", { text: false }).includes("Library"));
});

test("a page that can't be read still gives the app's process", () => {
  assert.equal(
    formatDump(new Error("the app exited (1)"), "app: pid 9 exited"),
    "(couldn't read the page: the app exited (1))\napp: pid 9 exited",
  );
});

test("the app's process: running, exited when asked, or on its own", () => {
  assert.equal(appLine(null), "app: its process isn't known here");
  assert.equal(appLine({ pid: 5, closedAt: null, exitedAt: null }), "app: pid 5, running");
  assert.equal(
    appLine({ pid: 5, closedAt: 1000, exitedAt: null }, 4000),
    "app: pid 5, running, 3000 ms after it was asked to quit",
  );
  const at = Date.UTC(2026, 9, 23, 12, 0, 1, 234);
  assert.equal(
    appLine({ pid: 5, closedAt: at - 830, exitedAt: at, exit: "code 0" }),
    "app: pid 5 exited (code 0) at 12:00:01.234 UTC, 830 ms after it was asked to quit",
  );
  assert.equal(
    appLine({ pid: 5, closedAt: null, exitedAt: at, exit: "signal SIGSEGV" }),
    "app: pid 5 exited (signal SIGSEGV) at 12:00:01.234 UTC, without being asked to quit",
  );
});
