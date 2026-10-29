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
