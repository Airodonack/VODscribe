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

// insideDrag.ts: "A native leave: whether it was the page's own drag."
test("a native leave says whether it was the page's own drag", () => {
  const drag = new InsideDrag();
  assert.equal(drag.leave(), false, "no drag of the page's");
  drag.start(LINK);
  assert.equal(drag.leave(), true, "while its drag goes on");
  drag.enter([LINK]);
  assert.equal(drag.leave(), true, "after its native enter");
  drag.end();
  assert.equal(drag.leave(), false, "once it's over");
});

// insideDrag.ts: "a drag carried out of the window is closed by a leave before dragend".
test("a drag carried out of the window and let go there adds nothing and is forgotten", () => {
  const drag = new InsideDrag();
  drag.start(LINK);
  drag.enter([LINK]);
  drag.leave();
  drag.end();
  assert.equal(drag.enter([LINK]), false);
  assert.deepEqual(drag.drop([LINK]), [LINK]);
});

// insideDrag.ts `drop`: "those to add (none from the page's own drag, while it goes on)".
test("a drop while the page's drag goes on adds nothing, even of what it didn't carry", () => {
  const drag = new InsideDrag();
  drag.start(LINK);
  drag.enter([LINK]);
  assert.deepEqual(drag.drop(["/home/me/other.mp4"]), []);
});

// insideDrag.ts: the drag's text is "a line each".
test("the drag's text is a line each: blank lines are none, and a line's edges don't count", () => {
  const drag = new InsideDrag();
  drag.start(`\n  ${LINK}  \r\n\r\nhttps://example.com/two\n`);
  drag.end();
  assert.equal(drag.enter([LINK, " https://example.com/two "]), true);
  assert.deepEqual(drag.drop([LINK, "https://example.com/two"]), []);
});

// insideDrag.ts `enter`: "One that comes after dragend is the drag's only if it carries what the drag did".
test("a native enter after dragend carrying more than the drag did, or nothing, isn't the page's", () => {
  const more = new InsideDrag();
  more.start(LINK);
  more.end();
  assert.equal(more.enter([LINK, "/home/me/video.mp4"]), false);
  const none = new InsideDrag();
  none.start(LINK);
  none.end();
  assert.equal(none.enter([]), false);
});

// insideDrag.ts: "A native enter after that is another drag's, from outside, and the same link dropped by it is added."
test("a drag from outside over the window while the page's own goes on is the page's", () => {
  const drag = new InsideDrag();
  drag.start(LINK);
  assert.equal(drag.enter(["/home/me/video.mp4"]), true);
});

// insideDrag.ts: "a drag shorter than that delay has its native enter and drop arrive after dragend".
test("a native drop with no enter before it adds what it carries, as no page drag is going", () => {
  const drag = new InsideDrag();
  assert.deepEqual(drag.drop(["/home/me/video.mp4", LINK]), ["/home/me/video.mp4", LINK]);
});

// insideDrag.ts: "the first native enter after dragstart is the drag's".
test("a second page drag is told apart from the first after the first closed", () => {
  const drag = new InsideDrag();
  drag.start(LINK);
  drag.enter([LINK]);
  drag.drop([LINK]);
  drag.end();
  drag.start(LINK);
  assert.equal(drag.enter([LINK]), true);
  assert.deepEqual(drag.drop([LINK]), []);
  drag.end();
  assert.deepEqual(added(["enter", "drop"], drag), [LINK]);
});

// insideDrag.ts: the drag's text is "a line each" (a line feed alone ends one too).
test("lines ended by a line feed alone are each carried", () => {
  const drag = new InsideDrag();
  drag.start(`${LINK}\nhttps://example.com/two`);
  drag.end();
  assert.equal(drag.enter([LINK, "https://example.com/two"]), true);
});

// insideDrag.ts: blank lines are no line of what's carried.
test("an empty path is not what the drag carried, however many blank lines it had", () => {
  const drag = new InsideDrag();
  drag.start(`${LINK}\n\n`);
  drag.end();
  assert.equal(drag.enter([""]), false);
});

// insideDrag.ts `drop`: "none from the page's own drag": by the line, edges not counting.
test("a path with spaces round it, dropped by the page's own drag, is still its own", () => {
  const drag = new InsideDrag();
  drag.start(LINK);
  drag.enter([LINK]);
  drag.end();
  assert.deepEqual(drag.drop([` ${LINK} `]), []);
});

// insideDrag.ts `enter`: "A native enter after that is another drag's, from outside".
test("a native enter that isn't the page's drag lets it go: even its own link after that is from outside", () => {
  const drag = new InsideDrag();
  drag.start(LINK);
  drag.end();
  assert.equal(drag.enter(["/home/me/video.mp4"]), false);
  assert.equal(drag.enter([LINK]), false);
  assert.deepEqual(drag.drop([LINK]), [LINK]);
});

// insideDrag.ts `enter`: it's over once closed by a drop or a leave; until then a second enter is still the drag's.
test("a second native enter while the first is open is still the page's drag, after dragend too", () => {
  const drag = new InsideDrag();
  drag.start(LINK);
  drag.enter([LINK]);
  drag.end();
  assert.equal(drag.enter(["/home/me/video.mp4"]), true);
});

// insideDrag.ts: "it's over once that is closed by a drop or a leave and dragend has come".
test("a leave after dragend closes the drag: the next enter is from outside", () => {
  const drag = new InsideDrag();
  drag.start(LINK);
  drag.enter([LINK]);
  drag.end();
  assert.equal(drag.leave(), true);
  assert.equal(drag.enter([LINK]), false);
  assert.deepEqual(drag.drop([LINK]), [LINK]);
});
