// Which native drops come from a drag that started inside the page (insideDrag.ts): node --test apps/desktop/src/lib/insideDrag.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { InsideDrag } from "./insideDrag.ts";

const LINK = "https://www.twitch.tv/somestreamer";

/**
 * The paths added from `events` ("start", "end", "enter", "leave", "drop"), each native one carrying LINK.
 * @param {string[]} events @param {InsideDrag} [drag]
 */
function added(events, drag = new InsideDrag()) {
  const out = [];
  for (const e of events) {
    if (e === "start") drag.start(LINK);
    else if (e === "end") drag.end();
    else if (e === "enter") drag.enter([LINK]);
    else if (e === "leave") drag.leave();
    else if (e === "drop") out.push(...drag.drop([LINK]));
  }
  return out;
}

test("a slow drag inside, its native drop after dragend, adds nothing", () => {
  assert.deepEqual(added(["start", "enter", "end", "drop"]), []);
});

test("a quick drag inside, its native enter and drop both after dragend, adds nothing", () => {
  assert.deepEqual(added(["start", "end", "enter", "drop"]), []);
});

test("a drag inside, its native drop before dragend, adds nothing", () => {
  assert.deepEqual(added(["start", "enter", "drop", "end"]), []);
});

test("the same link dragged in from outside afterwards is added", () => {
  for (const before of [
    ["start", "enter", "end", "drop"],
    ["start", "end", "enter", "drop"],
    ["start", "enter", "drop", "end"],
    // Carried out of the window and let go there.
    ["start", "enter", "leave", "end"],
    // Out of the window, back in, let go inside.
    ["start", "enter", "leave", "enter", "end", "drop"],
  ]) {
    const drag = new InsideDrag();
    added(before, drag);
    assert.deepEqual(added(["enter", "drop"], drag), [LINK], before.join(" "));
  }
});

test("a drag from outside while none started inside is added", () => {
  assert.deepEqual(added(["enter", "drop"]), [LINK]);
});

test("the native enter says whether it's the page's own drag", () => {
  const drag = new InsideDrag();
  drag.start(LINK);
  drag.end();
  assert.equal(drag.enter([LINK]), true, "a quick drag's late enter");
  drag.drop([LINK]);
  assert.equal(drag.enter([LINK]), false, "the next one, from outside");
});

test("only what the drag carried is left out", () => {
  const drag = new InsideDrag();
  drag.start(`${LINK}\r\n`);
  drag.end();
  drag.enter([LINK]);
  assert.deepEqual(drag.drop([LINK, "https://example.com/other"]), ["https://example.com/other"]);
});

test("after a drag inside that the window never hears, the next one from outside is its own", () => {
  // A transcript text selection: no native events on Windows or Linux.
  const drag = new InsideDrag();
  drag.start("a few words of the transcript");
  drag.end();
  assert.equal(drag.enter(["/home/me/video.mp4"]), false, "a file dragged in next shows the overlay");
  assert.deepEqual(drag.drop(["/home/me/video.mp4"]), ["/home/me/video.mp4"]);
});
