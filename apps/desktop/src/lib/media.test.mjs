// The player's and timeline's arithmetic (media.ts): node --test apps/desktop/src/lib/media.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  columns,
  cueAt,
  DEFAULT_FPS,
  elementSound,
  PEAKS_RATE,
  edgeIndex,
  edgeShows,
  frameIndex,
  isUnplayable,
  unplayable,
  playedLength,
  playedSpans,
  frameStart,
  inEdit,
  inFrame,
  nudge,
  stepTime,
  stretchIn,
  stretches,
  tickStep,
  waveformGaps,
} from "./media.ts";

test("the caption up is the last one started, until it ends", () => {
  const cues = [
    { start: 1, end: 2, text: "a" },
    { start: 2, end: 3.5, text: "b" },
    { start: 5, end: 6, text: "c" },
  ];
  assert.equal(cueAt(cues, 0.5), null);
  assert.equal(cueAt(cues, 1)?.text, "a");
  assert.equal(cueAt(cues, 2)?.text, "b", "the next one takes over at its start");
  assert.equal(cueAt(cues, 4), null, "a gap between two");
  assert.equal(cueAt(cues, 5.9)?.text, "c");
  assert.equal(cueAt(cues, 6), null);
  assert.equal(cueAt([], 1), null);
});

test("each column is the loudest level in its stretch", () => {
  // Two seconds: twenty levels, into four columns of five.
  const levels = Uint8Array.from({ length: 20 }, (_, i) => (i === 7 ? 255 : i));
  assert.deepEqual(Array.from(columns(levels, 2, 4)), [4 / 255, 1, 14 / 255, 19 / 255].map(Math.fround));
  // More columns than levels: each still reads one.
  assert.equal(columns(levels, 2, 40)[39], Math.fround(19 / 255));
  // The video runs on past the levels (read before it was all here): silence.
  assert.deepEqual(Array.from(columns(levels, 4, 4)).slice(2), [0, 0]);
  assert.deepEqual(Array.from(columns(new Uint8Array(), 2, 3)), [0, 0, 0]);
});

test("frame steps land inside the frame and count from it", () => {
  const fps = 30;
  let t = 10;
  t = stepTime(t, 1, fps);
  assert.ok(Math.abs(t - 301.25 / 30) < 1e-9);
  for (let i = 0; i < 100; i++) t = stepTime(t, 1, fps);
  assert.equal(Math.floor(t * fps), 401, "no drift");
  t = stepTime(t, -1, fps);
  assert.equal(Math.floor(t * fps), 400);
  assert.equal(stepTime(0, -1, fps), 0);
  // 29.97: a time on a frame's start counts as that frame.
  assert.equal(Math.floor(stepTime(100 / 29.97, 1, 29.97) * 29.97), 101);
});

test("ticks are spaced for their labels", () => {
  assert.equal(tickStep(120, 800), 10, "two minutes: every 10 s");
  assert.equal(tickStep(30, 800), 5);
  assert.equal(tickStep(10 * 3600, 800), 3600);
  assert.equal(tickStep(100 * 3600, 400), 14400, "the longest step at most");
});

test("a mark goes where the frame on show starts", () => {
  assert.equal(frameStart(10.02, 30), 10);
  assert.equal(frameStart(10.04, 30), 301 / 30);
  assert.equal(frameStart(301 / 30, 30), 301 / 30, "a time on a frame's start is that frame");
  assert.equal(frameStart(0.01, 60), 0);
});

test("marks move a frame at a time, within the video", () => {
  assert.equal(nudge(10, 1, 30, 100), 301 / 30);
  assert.equal(nudge(10, -10, 30, 100), 290 / 30);
  assert.equal(nudge(0, -1, 30, 100), 0);
  assert.equal(nudge(100, 5, 30, 100), 100);
  // From a time a hair off the frame (stored as a float): still whole frames.
  assert.equal(nudge(301 / 30 + 1e-9, 1, 30, 100), 302 / 30);
});

test("cuts split the video into stretches", () => {
  assert.deepEqual(stretches([], 60), [{ start: 0, end: 60 }]);
  assert.deepEqual(stretches([30], 60), [
    { start: 0, end: 30 },
    { start: 30, end: 60 },
  ]);
  assert.deepEqual(stretches([40, 10, 10, 0, 60, 70], 60), [
    { start: 0, end: 10 },
    { start: 10, end: 40 },
    { start: 40, end: 60 },
  ]);
});

test("a seek to a cut lands a quarter of a frame into the frame it starts, short of the end", () => {
  assert.equal(inFrame(1, 4, 10), 1.0625);
  assert.equal(frameStart(inFrame(2 / 30, 30, 10), 30), 2 / 30);
  assert.equal(inFrame(10, 30, 10), 10);
});

test("an edit's piece plays its stretch, a sliver nothing, as the render does", () => {
  assert.deepEqual(playedSpans(0, 10), [[0, 10]]);
  assert.equal(playedLength({ start: 2, end: 10 }), 8);
  assert.deepEqual(playedSpans(5, 5.02), []);
});

test("the stretch the playhead is in, the last one at the very end", () => {
  assert.deepEqual(stretchIn([2], 5, 1), { start: 0, end: 2 });
  assert.deepEqual(stretchIn([2], 5, 2), { start: 2, end: 5 });
  // Played to the end (the time stops at the length), or a site's player past it.
  assert.deepEqual(stretchIn([2], 5, 5), { start: 2, end: 5 });
  assert.deepEqual(stretchIn([], 5, 5.3), { start: 0, end: 5 });
  assert.equal(stretchIn([], 0, 0), undefined);
});

test("a file that won't play says why, and is known as such", () => {
  assert.equal(unplayable({ code: 4 }), "This video's format can't be played here.");
  assert.equal(unplayable(null), "This video couldn't be played.");
  assert.ok(isUnplayable(unplayable({ code: 4 })) && isUnplayable(unplayable(undefined)));
  assert.ok(!isUnplayable("The file is missing."));
});

test("a frame's start counts as that frame; an edge is the nearest", () => {
  const fps = 30000 / 1001;
  for (const k of [0, 1, 719, 107892]) {
    assert.equal(frameIndex(k / fps, fps), k, `frame ${k}'s start`);
    assert.equal(frameIndex((k + 0.9) / fps, fps), k, `late in frame ${k}`);
    assert.equal(edgeIndex((k + 0.4) / fps, fps), k);
    assert.equal(edgeIndex((k + 0.6) / fps, fps), k + 1);
  }
});

test("a site VOD's waveform has gaps where nothing is transcribed or fetched", () => {
  /**
   * @param {number} start @param {number | null} end
   * @param {import("./api").Stretch["status"]} status @param {number | null} reached
   * @returns {import("./api").Stretch}
   */
  const stretch = (start, end, status, reached = null, audio = false) => ({
    id: 0,
    vod_id: 1,
    start,
    end,
    status,
    progress: 0,
    reached,
    error: null,
    model: null,
    audio,
  });
  // Nothing asked for: all of it.
  assert.deepEqual(waveformGaps([], 600), [{ start: 0, end: 600 }]);
  // All of it transcribed (a VOD from before stretches): none.
  assert.deepEqual(waveformGaps([stretch(0, null, "done")], 600), []);
  // Transcribed 2:00–3:00 and 2:30–4:00 (overlapping), and a request whose
  // audio is here: levels there.
  assert.deepEqual(
    waveformGaps([stretch(150, 240, "done"), stretch(120, 180, "done"), stretch(500, null, "transcribing")], 600),
    [
      { start: 0, end: 120 },
      { start: 240, end: 500 },
    ],
  );
  // A request paused or failed after its audio came: levels to its end, as
  // media.ts `waveformGaps` says ("all of a request whose audio is here");
  // one still waiting: none.
  assert.deepEqual(
    waveformGaps(
      [stretch(0, 300, "paused", 100, true), stretch(300, 400, "queued"), stretch(400, null, "error", 400, true)],
      600,
    ),
    [{ start: 300, end: 400 }],
  );
  // A request paused or failed before its audio came: levels only as far as
  // it has written lines.
  assert.deepEqual(
    waveformGaps([stretch(0, 300, "paused", 100), stretch(300, 400, "queued"), stretch(400, null, "error", 400)], 600),
    [{ start: 100, end: 600 }],
  );
  // Ends meeting within a level: no gap between them.
  assert.deepEqual(waveformGaps([stretch(0, 100, "done"), stretch(100.05, null, "done")], 600), []);
});

test("a stretch of an edit's timeline starts in the VOD of the piece it starts in", () => {
  const pieces = [
    { vod_id: 1, start: 10, end: 12 },
    { vod_id: 1, start: 14, end: 20 },
    { vod_id: 2, start: 100, end: 105 },
  ];
  assert.deepEqual(inEdit(pieces, 1, 1.5), { vodId: 1, start: 11, end: 11.5 });
  // Past the first piece, and no further than the piece it starts in.
  assert.deepEqual(inEdit(pieces, 3, 20), { vodId: 1, start: 15, end: 20 });
  // At a join: the next piece's start.
  assert.deepEqual(inEdit(pieces, 8, 9), { vodId: 2, start: 100, end: 101 });
  assert.equal(inEdit(pieces, 13, 14), undefined);
});

/**
 * 0.4.1 Fixes: "Videos at 23.976, 29.97 or 59.94 frames a second showed one
 * frame twice every few minutes, and stepping frame by frame drifted."
 */
test("stepping frame by frame at 23.976, 29.97 or 59.94 shows each frame once, never twice or skipped", () => {
  for (const fps of [24000 / 1001, 30000 / 1001, 60000 / 1001]) {
    // Two hours of frames, forward then all the way back.
    const frames = Math.round(2 * 3600 * fps);
    let t = 0;
    for (let k = 1; k <= frames; k++) {
      t = stepTime(t, 1, fps);
      assert.equal(frameIndex(t, fps), k, `${fps}: stepped forward to frame ${k}`);
    }
    for (let k = frames - 1; k >= 0; k--) {
      t = stepTime(t, -1, fps);
      assert.equal(frameIndex(t, fps), k, `${fps}: stepped back to frame ${k}`);
    }
  }
});

/**
 * 0.5.0: "Dragging a selection's edge shows that frame. For the end, it's the
 * last frame inside the selection, which the player shows too." (media.ts
 * `edgeShows`: "its start's frame, or for its end the frame before".)
 */
test("a selection's start edge shows its own frame and its end edge the last frame inside it", () => {
  const fps = 30;
  assert.equal(edgeShows("start", 10, fps), 10);
  assert.equal(edgeShows("end", 10, fps), 299 / 30, "an end on a frame's start: the frame before it");
  assert.equal(edgeShows("end", 301 / 30, fps), 10, "an end where the next frame starts: the one before it");
  assert.equal(edgeShows("end", 0, fps), 0, "no frame before the first");
});

/** "The tick step for `span` seconds drawn `width` pixels wide, labels at least 64 px apart" (media.ts `tickStep`). */
test("a tick step is the first that leaves 64 pixels for its label", () => {
  assert.equal(tickStep(100, 640), 10, "10 s of 100 over 640 px is exactly 64 px");
  assert.equal(tickStep(100, 639), 15);
  assert.equal(tickStep(1, 64), 1, "the first step, when it fits");
});

/**
 * media.ts `inEdit`: "Where the stretch `from`..`to` of an edit's timeline
 * starts in its VOD, for pieces played end to end: the VOD and the stretch of
 * it from there, no further than the span it starts in. Undefined past the
 * edit's end."
 */
test("a stretch of an edit's timeline starts where the piece it falls in plays it", () => {
  const pieces = [
    { vod_id: 1, start: 10, end: 20 },
    { vod_id: 2, start: 100, end: 105 },
  ];
  assert.deepEqual(inEdit(pieces, 0, 4), { vodId: 1, start: 10, end: 14 });
  assert.deepEqual(inEdit(pieces, 4, 40), { vodId: 1, start: 14, end: 20 }, "no further than the piece's end");
  assert.deepEqual(inEdit(pieces, 10, 12), { vodId: 2, start: 100, end: 102 }, "at the join: the next piece");
  assert.deepEqual(inEdit(pieces, 14, 15), { vodId: 2, start: 104, end: 105 });
  assert.equal(inEdit(pieces, 15, 16), undefined, "past the edit's end");
  assert.equal(inEdit([], 0, 1), undefined);
});

/** media.ts `playedSpans`' and `playedLength`'s doc: what plays of `start`..`end`. */
test("an edit's piece plays from its start to its end", () => {
  assert.deepEqual(playedSpans(2, 9), [[2, 9]]);
  assert.equal(playedLength({ start: 2, end: 9 }), 7);
});

/** media.ts `columns`: "Levels past the end of `levels` are 0"; no levels or no duration: silence. */
test("a waveform with no levels, no length or no columns is silence", () => {
  const levels = Uint8Array.from([255, 255, 255, 255]);
  assert.deepEqual(Array.from(columns(levels, 0, 3)), [0, 0, 0], "no length");
  assert.deepEqual(Array.from(columns(levels, Number.NaN, 2)), [0, 0]);
  assert.equal(columns(levels, 2, 0).length, 0, "no columns");
  assert.equal(columns(levels, 2, -3).length, 0, "negative columns");
});

/** media.ts `columns`: "each the loudest level (0 to 1) in its stretch of `duration` seconds". */
test("a column's level is the loudest of its stretch, at the peaks' rate", () => {
  assert.equal(PEAKS_RATE, 10, "ten levels a second");
  // One second: ten levels, into two columns of five; the loudest is last in each.
  const levels = Uint8Array.from([0, 0, 0, 0, 51, 0, 0, 0, 0, 255]);
  assert.deepEqual(Array.from(columns(levels, 1, 2)), [0.2, 1].map(Math.fround));
});

/** media.ts `DEFAULT_FPS`: "The frame rate assumed until a player says (a site's player never does)". */
test("until a player says its frame rate, 30 is assumed", () => {
  assert.equal(DEFAULT_FPS, 30);
});

/** 0.4.1 (frame steps): a step before the start stays at the start, back from any frame in the first. */
test("a step back from the first frame stays at the start", () => {
  assert.equal(stepTime(0.01, -1, 30), 0);
  assert.equal(stepTime(0, 0, 30), 0.25 / 30, "no step: a quarter into the frame it's in");
});

/**
 * media.ts `tickStep`: "the first step that leaves room for a label"; the
 * longest step when none does.
 */
test("a short video's ticks are a second apart, a very long one's the longest step", () => {
  assert.equal(tickStep(1, 10_000), 1);
  assert.equal(tickStep(1e9, 100), 14400);
});

/**
 * media.ts `elementSound`: "A media element's volume control, muting by
 * setting the volume to 0 and back: WebKit lets an element be unmuted
 * (`muted = false`) only from a click, so a key (Shift+M) could mute but
 * never unmute it."
 */
function sound(volume = 1) {
  const el = { volume, muted: false };
  const saved = /** @type {number[]} */ ([]);
  const s = elementSound(
    () => /** @type {any} */ (el),
    (v) => saved.push(v),
  );
  return { el, saved, s };
}

test("muting sets the element's volume to 0 and unmuting puts it back, never touching `muted = true`", () => {
  const { el, s } = sound(0.6);
  s.setMuted(true);
  assert.equal(el.volume, 0);
  assert.equal(el.muted, false, "the element is never muted itself");
  assert.equal(s.muted(), true);
  assert.equal(s.get(), 0.6, "the bar still shows the level to come back to");
  s.setMuted(false);
  assert.equal(el.volume, 0.6);
  assert.equal(s.muted(), false);
  assert.equal(s.get(), 0.6);
});

test("muting while muted, or unmuting while not, changes nothing", () => {
  const { el, s } = sound(0.6);
  s.setMuted(false);
  assert.equal(el.volume, 0.6);
  s.setMuted(true);
  el.volume = 0.3; // not the sound's doing: muting again must not take this as the level
  s.setMuted(true);
  s.setMuted(false);
  assert.equal(el.volume, 0.6, "back to the level it was muted from");
});

test("a volume set is the element's, unmuted, and handed on to be saved", () => {
  const { el, saved, s } = sound(1);
  s.setMuted(true);
  s.set(0.4);
  assert.equal(el.volume, 0.4);
  assert.equal(s.muted(), false, "a volume above 0 unmutes");
  assert.equal(s.get(), 0.4);
  assert.deepEqual(saved, [0.4]);
});

test("a volume set to 0 is a mute", () => {
  const { el, saved, s } = sound(1);
  s.set(0);
  assert.equal(s.muted(), true);
  assert.deepEqual(saved, [0]);
  assert.equal(el.volume, 0);
});

test("a volume control with no element yet does nothing and reads full", () => {
  const s = elementSound(() => undefined);
  s.set(0.2);
  s.setMuted(true);
  assert.equal(s.muted(), false);
  assert.equal(s.get(), 1);
});

/** media.ts `columns`: more columns than levels still read the level at each. */
test("with more columns than levels, each column reads the level it falls on", () => {
  const levels = Uint8Array.from({ length: 20 }, (_, i) => i);
  const out = columns(levels, 2, 40);
  assert.equal(out[2], Math.fround(1 / 255));
  assert.equal(out[3], Math.fround(1 / 255));
  assert.equal(out[4], Math.fround(2 / 255));
});

/** media.ts `waveformGaps`: a request that's ready, or whose audio is here, is heard in all of it; "less than a level apart: no gap". */
test("a ready stretch is heard in full, and a gap of a level is a gap", () => {
  /** @param {number} start @param {number} end @param {import("./api").Stretch["status"]} status @returns {import("./api").Stretch} */
  const stretch = (start, end, status) => ({
    id: 0,
    vod_id: 1,
    start,
    end,
    status,
    progress: 0,
    reached: null,
    error: null,
    model: null,
    audio: false,
  });
  assert.deepEqual(waveformGaps([stretch(2, 3, "ready")], 4), [
    { start: 0, end: 2 },
    { start: 3, end: 4 },
  ]);
  assert.deepEqual(waveformGaps([stretch(0.1, 1, "done")], 1), [{ start: 0, end: 0.1 }]);
  assert.deepEqual(waveformGaps([], 0.1), [{ start: 0, end: 0.1 }]);
  assert.deepEqual(waveformGaps([stretch(0.09, 1, "done")], 1), []);
  assert.deepEqual(waveformGaps([stretch(0, 1, "done")], 1.09), []);
});

/** media.ts `stretchIn`: a time before the start is in none. */
test("a time before the start is in no stretch", () => {
  assert.equal(stretchIn([2], 5, -1), undefined);
});

/** media.ts `playedSpans`: "nothing if it's a sliver under 0.04 s". */
test("a piece of exactly 0.04 s plays", () => {
  assert.deepEqual(playedSpans(0, 0.04), [[0, 0.04]]);
});

test("a stretch starting where a piece's played time ends is in the next piece", () => {
  const pieces = [
    { vod_id: 1, start: 0, end: 10 },
    { vod_id: 2, start: 100, end: 105 },
  ];
  const found = inEdit(pieces, 10 - 1e-6, 11);
  assert.equal(found?.vodId, 2);
  assert.equal(found?.start, 100);
});

test("an unmuting set unmutes the element itself", () => {
  const el = { volume: 0, muted: true };
  const s = elementSound(() => /** @type {any} */ (el));
  s.set(0.5);
  assert.equal(el.muted, false);
  s.setMuted(true);
  s.setMuted(false);
  assert.equal(el.muted, false);
});
