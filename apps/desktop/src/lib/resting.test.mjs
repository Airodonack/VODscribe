// The pointer coming to rest (resting.ts): node --test apps/desktop/src/lib/resting.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { IDLE, resting } from "./resting.ts";

// A clock of its own: timers run when it's moved past them.
function clock() {
  let now = 0;
  /** @type {{ at: number, run: () => void }[]} */
  let due = [];
  return {
    timers: {
      /** @param {() => void} run @param {number} ms */
      set: (run, ms) => {
        const timer = { at: now + ms, run };
        due.push(timer);
        return timer;
      },
      /** @param {unknown} timer */
      clear: (timer) => void (due = due.filter((d) => d !== timer)),
    },
    /** @param {number} t */
    to(t) {
      now = t;
      for (const d of due.filter((d) => d.at <= t)) {
        due = due.filter((x) => x !== d);
        d.run();
      }
    },
  };
}

/** Moves along `path` ([x, y, t] each), the clock with them. */
function along(
  /** @type {import("./resting.ts").Rest} */ rest,
  /** @type {ReturnType<typeof clock>} */ c,
  /** @type {number[][]} */ path,
) {
  for (const [x, y, t] of path) {
    c.to(t);
    rest.move(x, y, t);
  }
}

test("a pointer crossing on its way elsewhere never rests", () => {
  const c = clock();
  let rests = 0;
  const rest = resting(() => rests++, { timers: c.timers });
  // 1 px a millisecond, a move each 16 ms, then gone.
  along(
    rest,
    c,
    [0, 1, 2, 3, 4, 5].map((i) => [i * 16, 10, i * 16]),
  );
  rest.leave();
  c.to(1000);
  assert.equal(rests, 0);
});

test("a pointer slowing down rests, once", () => {
  const c = clock();
  let rests = 0;
  const rest = resting(() => rests++, { timers: c.timers });
  // Fast, then 1 px per 16 ms (0.06 px/ms): rests on the third slow move.
  along(rest, c, [
    [0, 0, 0],
    [16, 0, 16],
    [32, 0, 32],
    [33, 0, 48],
    [34, 0, 64],
  ]);
  assert.equal(rests, 0, "a fast move is still among the last three");
  along(rest, c, [[35, 0, 80]]);
  assert.equal(rests, 1);
  along(rest, c, [
    [37, 0, 112],
    [38, 0, 128],
  ]);
  c.to(1000);
  assert.equal(rests, 1);
});

test("a pointer that stops rests after a moment", () => {
  const c = clock();
  let rests = 0;
  const rest = resting(() => rests++, { timers: c.timers });
  along(rest, c, [
    [0, 0, 0],
    [20, 0, 16],
  ]);
  c.to(16 + IDLE - 1);
  assert.equal(rests, 0);
  c.to(16 + IDLE);
  assert.equal(rests, 1);
});

test("leaving cancels, and coming back can rest again", () => {
  const c = clock();
  let rests = 0;
  const rest = resting(() => rests++, { timers: c.timers });
  along(rest, c, [[0, 0, 0]]);
  rest.leave();
  c.to(500);
  assert.equal(rests, 0);
  along(rest, c, [[5, 5, 600]]);
  c.to(800);
  assert.equal(rests, 1);
  rest.leave();
  along(rest, c, [[5, 5, 900]]);
  c.to(1100);
  assert.equal(rests, 2);
});

// resting.ts `restOver`: "Calls `onrest` when the pointer comes to rest over `node` (with no button held: a press or drag is no rest)".
test("over an element, a slow pointer rests, but not with a button held, nor after a press, nor once destroyed", async () => {
  const { restOver } = await import("./resting.ts");
  const node = new EventTarget();
  let first = 0;
  let second = 0;
  const action = restOver(/** @type {any} */ (node), () => first++);
  /** @param {string} type @param {number} x @param {number} t @param {number} [buttons] */
  const send = (type, x, t, buttons = 0) => {
    const e = new Event(type);
    Object.defineProperties(e, {
      clientX: { value: x },
      clientY: { value: 0 },
      timeStamp: { value: t },
      buttons: { value: buttons },
    });
    node.dispatchEvent(e);
  };
  // 0.05 px/ms: slow, but dragging.
  for (let i = 0; i < 6; i++) send("pointermove", i, i * 100, 1);
  assert.equal(first, 0);
  for (let i = 0; i < 6; i++) send("pointermove", i, 1000 + i * 100);
  assert.equal(first, 1);
  send("pointerleave", 0, 0);
  for (let i = 0; i < 6; i++) send("pointermove", i, 2000 + i * 100);
  assert.equal(first, 2, "rests again after the pointer left");
  send("pointerleave", 0, 0);
  // A press between the moves starts the count again.
  for (let i = 0; i < 2; i++) send("pointermove", i, 3000 + i * 100);
  send("pointerdown", 0, 0);
  for (let i = 0; i < 2; i++) send("pointermove", i, 3200 + i * 100);
  assert.equal(first, 2, "two and two moves aren't four");
  send("pointerleave", 0, 0);
  // The callback can be replaced.
  action.update(() => second++);
  for (let i = 0; i < 6; i++) send("pointermove", i, 4000 + i * 100);
  assert.deepEqual([first, second], [2, 1]);
  send("pointerleave", 0, 0);
  action.destroy();
  for (let i = 0; i < 6; i++) send("pointermove", i, 5000 + i * 100);
  assert.deepEqual([first, second], [2, 1], "nothing once it's destroyed");
});

// resting.ts `resting`: no move for IDLE ms is resting too, by the page's own timers.
test("a pointer that stops rests after a moment, by the page's own timers", async () => {
  /** @type {() => void} */
  let done = () => {};
  const rested = new Promise((resolve) => (done = () => resolve(undefined)));
  const rest = resting(done);
  rest.move(0, 0, 0);
  rest.move(20, 0, 16);
  await rested;
});

// resting.ts `resting`: only "slower than SPEED" rests, and a clock gone backwards isn't a speed.
test("a pointer exactly at the speed doesn't rest, nor one with its times going backwards", () => {
  const c = clock();
  let rests = 0;
  const rest = resting(() => rests++, { timers: c.timers });
  // Exactly 0.1 px/ms.
  along(
    rest,
    c,
    [0, 1, 2, 3, 4, 5].map((i) => [i, 0, i * 10]),
  );
  assert.equal(rests, 0);
  rest.leave();
  rest.move(0, 0, 100);
  rest.move(1, 0, 90);
  rest.move(2, 0, 80);
  rest.move(3, 0, 70);
  assert.equal(rests, 0);
});

// resting.ts `resting`: the path is the pointer's own, up and down as well as across.
test("a slow pointer rests wherever it is on the screen", () => {
  const c = clock();
  let rests = 0;
  const rest = resting(() => rests++, { timers: c.timers });
  along(
    rest,
    c,
    [0, 1, 2, 3, 4].map((i) => [300, 500 + i * 0.8, i * 10]),
  );
  assert.equal(rests, 1);
});

// resting.ts `resting`: "moving slower than SPEED across its last SAMPLES moves".
test("a pointer moving just under the speed rests, and just over it doesn't", () => {
  const c = clock();
  let rests = 0;
  const rest = resting(() => rests++, { timers: c.timers });
  // 0.12 px/ms (1.2 px each 10 ms) over the last three moves.
  along(
    rest,
    c,
    [0, 1, 2, 3, 4, 5].map((i) => [i * 1.2, 0, i * 10]),
  );
  assert.equal(rests, 0);
  rest.leave();
  // 0.08 px/ms.
  along(
    rest,
    c,
    [0, 1, 2, 3, 4, 5].map((i) => [i * 0.8, 0, 100 + i * 10]),
  );
  assert.equal(rests, 1);
});

// resting.ts `SAMPLES`: "moving slower than SPEED across its last SAMPLES moves": the moves are counted between points, so it takes one more point than that.
test("a slow pointer rests on its fourth move, not its third", () => {
  const c = clock();
  let rests = 0;
  const rest = resting(() => rests++, { timers: c.timers });
  along(
    rest,
    c,
    [0, 1, 2].map((i) => [i, 0, i * 16]),
  );
  assert.equal(rests, 0);
  along(rest, c, [[3, 0, 48]]);
  assert.equal(rests, 1);
});
