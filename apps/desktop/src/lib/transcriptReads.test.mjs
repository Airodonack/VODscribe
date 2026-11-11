// The transcript panel's reads (transcriptReads.ts): node --test apps/desktop/src/lib/transcriptReads.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { transcriptReads } from "./transcriptReads.ts";

const line = (/** @type {number} */ start) => ({ start, end: start + 1, text: `line ${start}` });
const tick = () => new Promise((r) => setTimeout(r));
const upTo = (/** @type {number} */ n) => Array.from({ length: n }, (_, i) => line(i));

/** A library the worker writes lines into, and a panel reading it: each read
 *  takes the lines as they are when it's made, and is answered when the test
 *  says (`answer`), so the test picks the order the answers land in. */
function panel() {
  let library = upTo(10);
  /** @type {{ kind: string, resolve: () => void, reject: (e: Error) => void }[]} */
  const pending = [];
  /** @type {{ lines: any[], error: unknown }} */
  const state = { lines: [], error: null };
  const reads = transcriptReads({
    lines: () => state.lines,
    show: (r) => {
      state.lines = r.lines;
      state.error = null;
    },
    failed: (e) => (state.error = e),
  });
  /** A read of `kind` taking `pick` of the library when made. */
  const reader = (/** @type {string} */ kind, /** @type {(l: any[]) => any[]} */ pick) => () => {
    const lines = pick(library);
    return new Promise((resolve, reject) =>
      pending.push({ kind, resolve: () => resolve({ lines, stretches: [] }), reject }),
    );
  };
  return {
    state,
    pending,
    whole: () => reads.whole(reader("whole", (l) => l)),
    range: (/** @type {number} */ start, /** @type {number | null} */ end) =>
      reads.range(
        start,
        end,
        reader("range", (l) => l.filter((x) => x.start >= start && (end === null || x.start < end))),
      ),
    write: (/** @type {any[]} */ lines) =>
      (library = [...library.filter((l) => !lines.some((n) => n.start === l.start)), ...lines].sort(
        (a, b) => a.start - b.start,
      )),
    /** Answers the reads made so far, and those made meanwhile, `first` kind first each round. */
    async answer(/** @type {string} */ first) {
      while (pending.length) {
        const i = Math.max(
          0,
          pending.findIndex((p) => p.kind === first),
        );
        pending.splice(i, 1)[0].resolve();
        await tick();
      }
    },
  };
}

// Quality report 0.7.4 item 3: "a full answer read earlier lands after a range
// answer" and "newer lines vanish from the transcript until the next 5% step".
test("lines written while the whole transcript is read stay shown when a range read lands first", async () => {
  const p = panel();
  p.whole();
  await tick();
  p.write([10, 11, 12, 13, 14].map(line));
  p.range(0, null);
  await p.answer("range");
  assert.equal(p.state.lines.length, 15);
});

test("a range read puts its lines in place of those in its range, keeping the rest", async () => {
  const p = panel();
  p.whole();
  await p.answer("whole");
  p.write([{ start: 4, end: 5, text: "new" }]);
  p.range(3, 6);
  await p.answer("range");
  assert.deepEqual(
    p.state.lines.map((l) => l.text),
    ["line 0", "line 1", "line 2", "line 3", "new", "line 5", "line 6", "line 7", "line 8", "line 9"],
  );
});

test("a whole read asked later wins over an earlier one that lands last", async () => {
  const p = panel();
  p.whole();
  await tick();
  p.write([10, 11].map(line));
  p.whole();
  // The newer one first, if both are under way.
  while (p.pending.length) {
    p.pending.pop()?.resolve();
    await tick();
  }
  assert.equal(p.state.lines.length, 12);
});

test("a range read asked before a whole read doesn't land over it", async () => {
  const p = panel();
  p.whole();
  await p.answer("whole");
  p.range(0, null);
  await tick();
  p.write([10].map(line));
  p.whole();
  await p.answer("whole");
  assert.equal(p.state.lines.length, 11);
});

test("a read that failed is told only while it's still the one wanted", async () => {
  const p = panel();
  p.whole();
  await tick();
  p.whole();
  p.pending.shift()?.reject(new Error("old"));
  await tick();
  await p.answer("whole");
  assert.equal(p.state.error, null);
  p.range(0, null);
  await tick();
  p.pending.shift()?.reject(new Error("range"));
  await tick();
  assert.equal(String(p.state.error), "Error: range");
});
