// Seeks that land (mediaSeek.ts), against what WebKit's media elements were
// seen doing: node --test apps/desktop/src/lib/mediaSeek.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { landingSeeks } from "./mediaSeek.ts";

/** A media element as far as landingSeeks uses it: its time, how much it
 *  has (`readyState`), and its events, sent by the test. */
class Media extends EventTarget {
  currentTime = 0;
  readyState = 0;
  seeking = false;
  /** Each time asked of it. @type {number[]} */
  asked = [];
  constructor() {
    super();
    const self = this;
    let time = 0;
    Object.defineProperty(this, "currentTime", {
      get: () => time,
      set(t) {
        self.asked.push(t);
        time = t;
      },
    });
    /** Where the element itself puts its time (not asked). @param {number} t */
    this.goes = (t) => (time = t);
  }
  /** @param {string} type */
  send(type) {
    this.dispatchEvent(new Event(type));
  }
}

test("a seek that lands is let go of", () => {
  const v = new Media();
  v.readyState = 4;
  const seeks = landingSeeks(/** @type {any} */ (v));
  seeks.seek(321.84);
  v.send("seeked");
  assert.equal(seeks.wanted(), null);
  assert.deepEqual(v.asked, [321.84]);
});

test("macOS: a seek said done before the first data, then put back at 0 as the data comes in, is asked again", () => {
  // The ten-minute mp3 opened by a click halfway along the timeline, as the
  // hosted Mac and a Mac runner showed it: seeked "there" at readyState 1,
  // then, with no seek of its own, at 0 once the data is in (loadeddata).
  const v = new Media();
  const seeks = landingSeeks(/** @type {any} */ (v));
  seeks.seek(321.84);
  v.readyState = 1;
  v.send("loadedmetadata");
  v.send("seeking");
  v.send("seeked");
  assert.equal(seeks.wanted(), 321.84, "let go of before the data was in");
  v.goes(0);
  v.readyState = 4;
  v.send("loadeddata");
  assert.equal(v.currentTime, 321.84, "left at 0 as the data came in");
  v.send("seeked");
  assert.equal(seeks.wanted(), null);
});

test("macOS: a seek said done before the first data and still there once it's in is let go of", () => {
  // As the same click on a Mac runner, other times: playing from there
  // shows the element's time, not the one asked, from then on.
  const v = new Media();
  const seeks = landingSeeks(/** @type {any} */ (v));
  seeks.seek(321.84);
  v.readyState = 1;
  v.send("seeked");
  v.readyState = 4;
  v.send("loadeddata");
  assert.equal(seeks.wanted(), null);
  assert.deepEqual(v.asked, [321.84]);
});

test("WebKitGTK: a seek that ends at 0 is asked again, a few times at most", () => {
  const v = new Media();
  v.readyState = 4;
  const seeks = landingSeeks(/** @type {any} */ (v));
  seeks.seek(100);
  for (let i = 0; i < 3; i++) {
    v.goes(0);
    v.send("seeked");
    assert.equal(v.currentTime, 100);
  }
  v.goes(0);
  v.send("seeked");
  assert.equal(seeks.wanted(), null, "asked again for ever");
  assert.equal(v.asked.length, 4);
});

/** mediaSeek.ts: "asked again when the metadata is in", "whatever the video says: it may say it's there and not be". */
test("a seek is asked again once the metadata is in, even if the video says it's there", () => {
  const v = new Media();
  const seeks = landingSeeks(/** @type {any} */ (v));
  seeks.seek(50);
  v.goes(50);
  v.send("loadedmetadata");
  assert.deepEqual(v.asked, [50, 50]);
});

/** mediaSeek.ts `landingSeeks`: nothing asked, nothing done. */
test("with no seek asked, the element's events leave its time alone", () => {
  const v = new Media();
  v.readyState = 4;
  const seeks = landingSeeks(/** @type {any} */ (v));
  v.goes(12);
  for (const type of ["loadedmetadata", "loadeddata", "seeked"]) v.send(type);
  assert.deepEqual(v.asked, []);
  assert.equal(seeks.wanted(), null);
});

/** mediaSeek.ts: "kept until a seek ends there, asked again ... after a seek that ends somewhere else (a few times)". */
test("a new seek starts the tries over", () => {
  const v = new Media();
  v.readyState = 4;
  const seeks = landingSeeks(/** @type {any} */ (v));
  seeks.seek(100);
  for (let i = 0; i < 3; i++) {
    v.goes(0);
    v.send("seeked");
  }
  seeks.seek(200);
  assert.equal(seeks.wanted(), 200);
  for (let i = 0; i < 3; i++) {
    v.goes(0);
    v.send("seeked");
    assert.equal(v.currentTime, 200, `try ${i + 1}`);
  }
});

/** mediaSeek.ts `landingSeeks`: "There with no seek still under way: landed". */
test("data in while a seek is still under way at the place keeps it wanted till it ends", () => {
  const v = new Media();
  v.readyState = 4;
  const seeks = landingSeeks(/** @type {any} */ (v));
  seeks.seek(30);
  v.seeking = true;
  v.send("loadeddata");
  assert.equal(seeks.wanted(), 30);
  v.seeking = false;
  v.send("seeked");
  assert.equal(seeks.wanted(), null);
});

/** mediaSeek.ts `landingSeeks`: a seek has landed when the element's time is within 0.05 s of it. */
test("a seek ending within a twentieth of a second of the time asked has landed", () => {
  const near = new Media();
  near.readyState = 4;
  const a = landingSeeks(/** @type {any} */ (near));
  a.seek(10);
  near.goes(10.04);
  near.send("seeked");
  assert.equal(a.wanted(), null);
  const far = new Media();
  far.readyState = 4;
  const b = landingSeeks(/** @type {any} */ (far));
  b.seek(10);
  far.goes(10.06);
  far.send("seeked");
  assert.equal(b.wanted(), 10, "asked again");
  assert.equal(far.currentTime, 10);
});

/** mediaSeek.ts `wanted`: "Where it's going, while a seek hasn't landed there yet." */
test("a seek is the wanted time at once, and the element is asked", () => {
  const v = new Media();
  const seeks = landingSeeks(/** @type {any} */ (v));
  assert.equal(seeks.wanted(), null);
  seeks.seek(0);
  assert.equal(seeks.wanted(), 0, "0 is a time too");
  assert.deepEqual(v.asked, [0]);
});

/** mediaSeek.ts `there`: "within 0.05 s": exactly that far is not there. */
test("a seek ending a twentieth of a second off has not landed", () => {
  const v = new Media();
  v.readyState = 4;
  const seeks = landingSeeks(/** @type {any} */ (v));
  seeks.seek(0);
  v.goes(0.05);
  v.send("seeked");
  assert.deepEqual(v.asked, [0, 0]);
});

/** mediaSeek.ts: "a seek that ends before then (the first data) isn't taken as landed": with the current frame's data in (HAVE_CURRENT_DATA), it is. */
test("a seek ending once the current frame's data is in has landed", () => {
  const v = new Media();
  v.readyState = 2;
  const seeks = landingSeeks(/** @type {any} */ (v));
  seeks.seek(7);
  v.send("seeked");
  assert.equal(seeks.wanted(), null);
});
