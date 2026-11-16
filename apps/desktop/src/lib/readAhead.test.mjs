// A VOD read before it's opened (readAhead.ts), through the page's own api.ts
// with the app faked: Tauri's invoke answers each read the test sets, and
// the reads asked are kept in order.
// node --test apps/desktop/src/lib/readAhead.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

const fakes = {
  "@tauri-apps/api/core": `export const invoke = (cmd, args) => globalThis.fake.invoke(cmd, args);`,
  "@tauri-apps/api/event": `export const listen = () => Promise.resolve(() => {});`,
};
const hooks = `
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (fakes[specifier]) return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.startsWith("file:"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}`;
register("data:text/javascript," + encodeURIComponent(hooks), import.meta.url);

/** What the app answers, by command; and each command asked, with its arguments, in order.
 *  @type {Record<string, (args: any) => unknown>} */
let answers = {};
/** @type {{ cmd: string, args: any }[]} */
let asked = [];
globalThis.fake = {
  /** @param {string} cmd @param {any} args */
  invoke(cmd, args) {
    asked.push({ cmd, args });
    const answer = answers[cmd];
    if (!answer) return Promise.reject(new Error(`the page asked ${cmd}`));
    try {
      return Promise.resolve(answer(args));
    } catch (e) {
      return Promise.reject(e);
    }
  },
};

const { readAhead } = await import("./readAhead.ts");
const { CHAT_BINS, CHAT_SHOWN } = await import("./rows.ts");

let nextId = 1000;
/** A VOD of its own (reads are kept per VOD id), with `chat` as given.
 *  @param {Partial<import("./api").Vod>} [over] @returns {import("./api").Vod} */
const vod = (over) => /** @type {any} */ ({ id: nextId++, chat: "done", duration: 300, transcribed: "none", ...over });
/** @param {number} n */
const lines = (n) => Array.from({ length: n }, (_, i) => ({ start: i * 5, text: `line ${i}` }));
/** A VOD's whole 100 s transcribed: no part left to ask for. */
const whole = () => [{ start: 0, end: null, status: "done", reached: null }];
/** Sets the app's answers for one test, with the usual ones under `own`.
 *  @param {Record<string, (args: any) => unknown>} [own] */
function app(own = {}) {
  asked = [];
  answers = {
    list_marks: () => [],
    transcript: () => [],
    transcript_stretches: () => [],
    chat_overview: () => ({ bins: [], events: [] }),
    chat_windows: ({ ranges }) => ranges.map(() => ({ messages: [], total: 0 })),
    ...own,
  };
}
/** The commands asked, by name. */
const names = () => asked.map((a) => a.cmd);

// Source for the tests: readAhead.ts's header ("Reads what opening a VOD
// reads, before it's opened ... Its lines and what of it is transcribed, its
// marks and, with its chat fetched, the chat's counts and the first rows'
// messages, asked as the transcript asks for them when it opens at the top").

test("a VOD with its chat fetched has its marks, lines, stretches, chat counts and first rows' chat read", async () => {
  app({ transcript: () => lines(3) });
  const v = vod();
  await readAhead(v);
  assert.deepEqual(
    names().sort(),
    ["chat_overview", "chat_windows", "list_marks", "transcript", "transcript_stretches"].sort(),
  );
  const overview = asked.find((a) => a.cmd === "chat_overview")?.args;
  assert.deepEqual(overview, { vodId: v.id, duration: 300, bins: CHAT_BINS, events: true });
});

test("the first rows' chat is asked a row at a time, each as many messages as the transcript shows at first", async () => {
  app({ transcript: () => lines(2), transcript_stretches: whole });
  const v = vod({ duration: 100, transcribed: "all" });
  await readAhead(v);
  const windows = asked.find((a) => a.cmd === "chat_windows")?.args;
  assert.equal(windows.vodId, v.id);
  assert.equal(windows.cap, CHAT_SHOWN);
  // Chat before the first line, the two lines' rows, then the minute after the last.
  assert.deepEqual(windows.ranges, [
    [-1e12, 0],
    [0, 5],
    [5, 60],
    [60, 1e12],
  ]);
});

test("only the rows a tall window shows at the top have their chat read", async () => {
  app({ transcript: () => lines(100) });
  await readAhead(vod({ duration: 600 }));
  const windows = asked.find((a) => a.cmd === "chat_windows")?.args;
  assert.equal(windows.ranges.length, 40);
});

test("a VOD whose chat isn't fetched has none of it read, and its lines still are", async () => {
  for (const chat of ["none", "fetching", "error"]) {
    app({ transcript: () => lines(3) });
    await readAhead(vod({ chat: /** @type {any} */ (chat) }));
    assert.deepEqual(names().sort(), ["list_marks", "transcript", "transcript_stretches"], chat);
  }
});

test("a part not transcribed has a row of its own among the first rows' chat, once some of the VOD is transcribed", async () => {
  app({
    transcript: () => lines(1),
    transcript_stretches: () => [{ start: 0, end: 10, status: "done", reached: null }],
  });
  await readAhead(vod({ duration: 100, transcribed: "some" }));
  const windows = asked.find((a) => a.cmd === "chat_windows")?.args;
  assert.deepEqual(windows.ranges.at(-3), [0, 10]);
  assert.ok(
    windows.ranges.some((/** @type {number[]} */ [from]) => from === 10),
    "the gap starts at 10",
  );
});

test("with no lines and nothing transcribed, the first rows are minutes of chat", async () => {
  app();
  await readAhead(vod({ duration: 130 }));
  const windows = asked.find((a) => a.cmd === "chat_windows")?.args;
  assert.deepEqual(windows.ranges, [
    [0, 60],
    [60, 120],
    [120, 1e12],
  ]);
});

test("a VOD of unknown length has no rows of chat to read", async () => {
  app();
  await readAhead(vod({ duration: null }));
  assert.ok(!names().includes("chat_windows"));
  assert.equal(asked.find((a) => a.cmd === "chat_overview")?.args.duration, 0);
});

test("a marks read that fails doesn't stop the rest", async () => {
  app({
    list_marks: () => {
      throw new Error("no");
    },
    transcript: () => lines(1),
  });
  await readAhead(vod());
  assert.ok(names().includes("chat_windows"));
});

test("a chat count that fails doesn't stop the first rows' chat", async () => {
  app({
    chat_overview: () => {
      throw new Error("no");
    },
    transcript: () => lines(1),
  });
  await readAhead(vod());
  assert.ok(names().includes("chat_windows"));
});

test("a lines read that fails fails the read ahead, and no chat is read", async () => {
  app({
    transcript: () => {
      throw new Error("no");
    },
  });
  await assert.rejects(readAhead(vod()));
  assert.ok(!names().includes("chat_windows"));
});

test("a VOD read ahead twice is read from the app once", async () => {
  app({ transcript: () => lines(2) });
  const v = vod();
  await readAhead(v);
  const first = asked.length;
  await readAhead(v);
  assert.equal(asked.length, first);
});
