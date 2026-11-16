// Adding VODs to the library (adder.svelte.ts), the page's store loaded as the
// app's build does, with the shell behind it faked: a link's lookup and a
// file's reading answer when the test says.
// node --test apps/desktop/src/lib/adder.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { loadStoresWithShell } from "./shellFake.mjs";

loadStoresWithShell();

/** @typedef {{ cmd: string, args: any, resolve: (v: any) => void, reject: (e: any) => void }} Call */

/** The shell's side of adding: every request is held until the test answers it. */
function shell() {
  /** @type {Call[]} */
  const waiting = [];
  /** Every request made, in order. @type {{ cmd: string, args: any }[]} */
  const asked = [];
  /** @type {any} */ (globalThis).shell = {
    listen: async () => () => {},
    invoke: (/** @type {string} */ cmd, /** @type {any} */ args) => {
      asked.push({ cmd, args });
      if (cmd === "cancel_add") {
        // A link being looked up stops.
        for (const c of waiting) if (c.cmd === "add_vod" && c.args.ticket === args.ticket) c.reject("Canceled");
        return Promise.resolve();
      }
      return new Promise((resolve, reject) => waiting.push({ cmd, args, resolve, reject }));
    },
  };
  /** Waits until the shell holds a request `cmd` (for `arg`, a url or a path), and takes it. */
  const request = async (/** @type {string} */ cmd, /** @type {string} */ arg) => {
    for (let turn = 0; turn < 1000; turn++) {
      const i = waiting.findIndex((c) => c.cmd === cmd && (c.args.url ?? c.args.paths?.[0]) === arg);
      if (i >= 0) return waiting.splice(i, 1)[0];
      await new Promise((r) => setImmediate(r));
    }
    throw new Error(`no ${cmd} ${arg} was asked for; asked: ${JSON.stringify(asked)}`);
  };
  return {
    asked,
    /** The VOD's id a link's lookup answers with. */
    async found(/** @type {string} */ url, /** @type {number} */ id) {
      (await request("add_vod", url)).resolve({ vod: { id }, created: true });
    },
    async failed(/** @type {string} */ url, /** @type {string} */ why) {
      (await request("add_vod", url)).reject(why);
    },
    async read(/** @type {string} */ path, /** @type {number[]} */ ids, /** @type {string[]} */ errors = []) {
      (await request("add_files", path)).resolve({ vods: ids.map((id) => ({ id })), errors });
    },
    async unreadable(/** @type {string} */ path, /** @type {string} */ why) {
      (await request("add_files", path)).reject(why);
    },
    /** @param {string} cmd @param {string} arg */
    held: (cmd, arg) => waiting.some((c) => c.cmd === cmd && (c.args.url ?? c.args.paths?.[0]) === arg),
  };
}

let copies = 0;
/** An adder of its own, and the ids it was told to open. */
async function adder() {
  const { Adder } = await import(`./adder.svelte.ts?copy=${copies++}`);
  /** @type {number[]} */
  const opened = [];
  /** @type {import("./adder.svelte.ts").Adder} */
  const a = new Adder((/** @type {number} */ id) => opened.push(id));
  return { a, opened };
}
const labels = (/** @type {import("./adder.svelte.ts").Adder} */ a) => a.pending.map((p) => p.label);
/** Waits for the first stand-in's turn. @param {import("./adder.svelte.ts").Adder} a */
async function started(a) {
  for (let turn = 0; turn < 1000 && !a.pending.at(-1)?.started; turn++) await idle();
  assert.ok(a.pending.at(-1)?.started, "its turn never came");
}
const idle = () => new Promise((r) => setImmediate(r));

// 0.3.0: "Links and files being added show in the library right away, with a spinner, and ✕ cancels one."
test("a link being added shows at once as a stand-in that starts on its turn, and is opened when it's in", async () => {
  const s = shell();
  const { a, opened } = await adder();
  const added = a.link("https://www.twitch.tv/videos/1");
  assert.deepEqual(labels(a), ["twitch.tv/videos/1"], "a link without its scheme and www");
  assert.equal(a.adding, true);
  await s.found("https://www.twitch.tv/videos/1", 7);
  assert.equal(await added, true);
  assert.deepEqual(a.pending, []);
  assert.equal(a.adding, false);
  assert.equal(a.error, null);
  assert.deepEqual(opened, [7]);
});

// adder.svelte.ts `Pending.started`: "Its turn has come (till then it waits behind the others)."
test("a stand-in waiting behind another isn't started till its turn", async () => {
  const s = shell();
  const { a } = await adder();
  void a.link("https://a.example/1");
  void a.link("https://b.example/2");
  await idle();
  assert.deepEqual(
    a.pending.map((p) => [p.label, !!p.started]),
    [
      ["b.example/2", false],
      ["a.example/1", true],
    ],
    "the newest first, as the library shows VODs",
  );
  assert.equal(s.held("add_vod", "https://b.example/2"), false, "its lookup isn't asked for yet");
  await s.found("https://a.example/1", 1);
  await idle();
  assert.deepEqual(labels(a), ["b.example/2"], "the one that came in is gone, the one waiting stays");
  await s.found("https://b.example/2", 2);
});

// adder.svelte.ts `Adder.link`: "its VOD's id, or null if it failed"; `error`: "What went wrong, one line per failure".
test("a link that fails says why, and isn't opened", async () => {
  const s = shell();
  const { a, opened } = await adder();
  const added = a.link("https://a.example/1");
  await s.failed("https://a.example/1", "Unsupported URL");
  assert.equal(await added, false);
  assert.equal(a.error, "Unsupported URL");
  assert.deepEqual(a.pending, []);
  assert.deepEqual(opened, []);
});

// 0.2.1: "A link pasted while dropped links or files are still being added waits its turn, and the errors of both show together, instead of the Add button coming back early."
test("a link pasted while others are added waits its turn, and the errors of both show together", async () => {
  const s = shell();
  const { a } = await adder();
  a.items(["https://a.example/1"], false);
  const pasted = a.link("https://b.example/2");
  await s.failed("https://a.example/1", "first failed");
  await idle();
  assert.equal(a.adding, true, "the Add button doesn't come back early");
  assert.equal(a.error, null, "nothing is shown till all are done");
  await s.failed("https://b.example/2", "second failed");
  assert.equal(await pasted, false);
  assert.equal(a.adding, false);
  assert.equal(a.error, "first failed\nsecond failed");
});

// adder.svelte.ts `Adder.#enqueue`: errors are of one run of the queue; the next starts with none.
test("what an earlier add reported is gone when the next one starts", async () => {
  const s = shell();
  const { a } = await adder();
  const first = a.link("https://a.example/1");
  await s.failed("https://a.example/1", "no");
  await first;
  assert.equal(a.error, "no");
  const second = a.link("https://b.example/2");
  assert.equal(a.error, null, "put away as the next is added");
  await s.found("https://b.example/2", 3);
  await second;
  assert.equal(a.error, null);
});

// adder.svelte.ts `Adder.items`: "Adds what was dropped or picked: the links in it".
test("a link is taken out of the text dropped, whatever spaces and words are round it", async () => {
  const s = shell();
  const { a, opened } = await adder();
  a.items(["", "  look at https://www.youtube.com/watch?v=abc now  ", "   "], false);
  assert.deepEqual(labels(a), ["youtube.com/watch?v=abc"]);
  await s.found("https://www.youtube.com/watch?v=abc", 4);
  await idle();
  assert.deepEqual(opened, [4]);
});

// adder.svelte.ts `Adder.items`: "(only from a real file drop or the file picker, `allowFiles`) the files."
test("text that isn't a link isn't a file unless files were dropped or picked", async () => {
  const s = shell();
  const { a } = await adder();
  const long = "x".repeat(100);
  a.items(["hello", long], false);
  await idle();
  assert.equal(a.error, `“hello” isn't a link.\n“${"x".repeat(80)}…” isn't a link.`);
  assert.deepEqual(s.asked, [], "nothing was asked of the app");
  assert.equal(a.adding, false);
});

// adder.svelte.ts `Adder.items`: nothing to add changes nothing.
test("nothing dropped, or only blanks, adds nothing and shows nothing", async () => {
  const s = shell();
  const { a } = await adder();
  a.error = "an earlier error";
  a.items([], true);
  a.items(["  ", ""], true);
  assert.equal(a.adding, false);
  assert.deepEqual(a.pending, []);
  assert.equal(a.error, "an earlier error");
  assert.deepEqual(s.asked, []);
});

// adder.svelte.ts header: "files are probed with ffmpeg"; `Adder.items`: "A file at a time".
test("files are added a file at a time, the first one opened, and what couldn't be added shown", async () => {
  const s = shell();
  const { a, opened } = await adder();
  a.items(["/videos/one.mp4", "/videos/two.mp4"], true);
  assert.deepEqual(labels(a), ["two.mp4", "one.mp4"]);
  await idle();
  assert.equal(s.held("add_files", "/videos/two.mp4"), false, "the second isn't read while the first is");
  await s.read("/videos/one.mp4", [11]);
  await s.read("/videos/two.mp4", [], ["two.mp4: no video in it"]);
  await idle();
  assert.deepEqual(opened, [11]);
  assert.equal(a.error, "two.mp4: no video in it");
  assert.deepEqual(a.pending, []);
});

// adder.svelte.ts header: "the first VOD of each add is opened (`onadded`), to watch, clip or transcribe."
test("of the links and files dropped together, only the first one added is opened", async () => {
  const s = shell();
  const { a, opened } = await adder();
  a.items(["https://a.example/1", "https://b.example/2", "/videos/c.mp4"], true);
  await s.failed("https://a.example/1", "no");
  await s.found("https://b.example/2", 22);
  await s.read("/videos/c.mp4", [33]);
  await idle();
  assert.deepEqual(opened, [22], "the first that came in, not the first asked");
  assert.equal(a.error, "no");
});

// 0.3.0: "✕ cancels one." adder.svelte.ts `Adder.cancel`: "one waiting is skipped".
test("a link waiting is canceled at once and never asked for", async () => {
  const s = shell();
  const { a, opened } = await adder();
  a.items(["https://a.example/1", "https://b.example/2"], false);
  const waiting = a.pending.find((p) => p.label === "b.example/2");
  a.cancel(/** @type {any} */ (waiting).key);
  assert.deepEqual(labels(a), ["a.example/1"]);
  await s.found("https://a.example/1", 5);
  await idle();
  assert.equal(
    s.asked.some((r) => r.cmd === "add_vod" && r.args.url === "https://b.example/2"),
    false,
  );
  assert.equal(a.error, null);
  assert.deepEqual(opened, [5]);
  assert.equal(a.adding, false);
});

// adder.svelte.ts `Adder.cancel`: "a link being looked up is stopped (its lookup can take a while)".
test("a link being looked up is stopped when canceled, and its stopping isn't an error", async () => {
  const s = shell();
  const { a, opened } = await adder();
  const added = a.link("https://a.example/1");
  await started(a);
  a.cancel(a.pending[0].key);
  assert.deepEqual(a.pending, []);
  assert.equal(await added, false);
  assert.equal(a.error, null);
  assert.deepEqual(opened, []);
  assert.equal(a.adding, false);
});

// adder.svelte.ts `Adder.cancel`: "A file being read finishes (in a second or so) and is added."
test("a file being read when canceled still finishes and is added", async () => {
  const s = shell();
  const { a, opened } = await adder();
  a.items(["/videos/one.mp4"], true);
  await started(a);
  a.cancel(a.pending[0].key);
  assert.deepEqual(a.pending, [], "its stand-in is gone at once");
  await s.read("/videos/one.mp4", [8]);
  await idle();
  assert.deepEqual(opened, [8]);
});

// adder.svelte.ts `Adder.items`: "A file at a time, so one canceled before its turn is skipped."
test("a file canceled before its turn is skipped", async () => {
  const s = shell();
  const { a, opened } = await adder();
  a.items(["/videos/one.mp4", "/videos/two.mp4"], true);
  a.cancel(/** @type {any} */ (a.pending.find((p) => p.label === "two.mp4")).key);
  await s.read("/videos/one.mp4", [1]);
  await idle();
  assert.equal(
    s.asked.some((r) => r.cmd === "add_files" && r.args.paths[0] === "/videos/two.mp4"),
    false,
  );
  assert.deepEqual(opened, [1]);
  assert.equal(a.adding, false);
});

// adder.svelte.ts `Adder.#nextKey`: "the add's ticket in the app, so it starts somewhere new each launch: a reloaded window's keys don't meet tickets left from before."
test("each launch of the page starts its stand-in keys somewhere new", async () => {
  shell();
  const first = await adder();
  const second = await adder();
  void first.a.link("https://a.example/1");
  void second.a.link("https://a.example/1");
  assert.notEqual(first.a.pending[0].key, second.a.pending[0].key);
  assert.ok(first.a.pending[0].key > 2 ** 20, "not from 0 up");
});

// adder.svelte.ts header: "Links go through the app's check"; `Adder.items`: `https?://`.
test("a link with http, in any case, is a link", async () => {
  const s = shell();
  const { a } = await adder();
  a.items(["see HTTP://a.example/1 and"], false);
  assert.deepEqual(labels(a), ["a.example/1"]);
  await s.found("HTTP://a.example/1", 1);
});

// adder.svelte.ts `Adder.items`: a name of eighty characters is shown whole, a longer one cut.
test("a name of exactly eighty characters in the error isn't cut", async () => {
  shell();
  const { a } = await adder();
  const name = "y".repeat(80);
  a.items([name], false);
  await idle();
  assert.equal(a.error, `“${name}” isn't a link.`);
});

// adder.svelte.ts `Adder.items`: a file the app can't read is a failure shown with the others.
test("a file the app fails to read is shown as an error", async () => {
  const s = shell();
  const { a, opened } = await adder();
  a.items(["/videos/one.mp4"], true);
  await s.unreadable("/videos/one.mp4", "disk gone");
  await idle();
  assert.equal(a.error, "disk gone");
  assert.deepEqual(opened, []);
  assert.deepEqual(a.pending, []);
});
