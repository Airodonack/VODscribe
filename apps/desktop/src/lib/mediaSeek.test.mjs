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
