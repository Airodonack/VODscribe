// Quitting, timed: from asking the app to quit to its process gone (and its
// window off the screen), in each state a person quits from, by the
// window's close (Alt+F4, the window manager's Close, as its close button
// does) and by the app's own exit (the test hook's quit, as a Quit in a
// menu would). Each is taken several times and logged with its median, and
// where the time went: the app writes each step of quitting to stderr with
// VODSCRIBE_TRACE_EXIT set (lib.rs, trace_exit), read here from the bridge
// driver's `proc.stderr`. No budget: the numbers are for comparing builds.
//
// Through the bridge driver only (--driver bridge): under WebDriver the
// driver ends the app from outside, so its own quit never runs. X11 only
// (xdotool). A site's VOD's embed isn't among the states: its player needs
// the network. E2E_EXIT_STATES=idle,playing picks states (a debug build's
// run, which is slower to start).

import { execFile, execFileSync } from "node:child_process";
import { copyFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { setTimeout as sleep } from "node:timers/promises";
import { promisify } from "node:util";

import { App, MINUTE, until } from "../lib/app.mjs";
import { Measures, median } from "../lib/budgets.mjs";
import { dataDirs } from "../lib/data.mjs";
import { longSpeech, longVideo, minute, TWITCH_VOD, videos } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";
import { leftovers } from "../lib/procs.mjs";

/** How many times each state is quit, each way. */
const TIMES = 5;
/** The step the app writes last before its process ends. */
const LAST_STEP = "quit's work done: the process ends";

/** How many windows of process `pid` are on the screen (xdotool finds none: 0). */
async function shownWindows(pid) {
  try {
    const { stdout } = await promisify(execFile)("xdotool", ["search", "--onlyvisible", "--pid", String(pid)]);
    return stdout.split("\n").filter(Boolean).length;
  } catch {
    return 0;
  }
}

/** The steps of the app's last quit in `stderr`: [{ step, at }], `at` in ms since the epoch. */
function traced(stderr) {
  const began = [...stderr.matchAll(/^exit trace: began at (\d+) ms since the epoch$/gm)].at(-1);
  if (!began) return [];
  return [...stderr.slice(began.index).matchAll(/^exit trace: ([\d.]+) ms (.*)$/gm)].map((m) => ({
    step: m[2],
    at: Number(began[1]) + Number(m[1]),
  }));
}

/**
 * Quits `app` `way` ("close" or "quit"); returns { gone, window, steps,
 * left }: ms from the ask to the process gone and to its last window off the
 * screen (null: not seen), each traced step's ms from the ask, and what the
 * app started still running after it.
 */
async function quitBy(way, app, ctx) {
  const proc = app.driver.proc;
  if (!(await shownWindows(proc.pid))) throw new Error("no window of the app's on the screen: the check can't work");
  if (way === "close") os.activate(proc);
  // The window looked for till it's gone, beside the quit.
  let window = null;
  const watching = (async () => {
    while (proc.exitedAt === null && window === null) {
      if (!(await shownWindows(proc.pid))) window = Date.now();
    }
  })();
  let asked;
  if (way === "close") {
    asked = proc.closedAt = Date.now();
    execFileSync("xdotool", ["key", "alt+F4"]);
    await until("the app to end", () => proc.exitedAt !== null, 30_000, { every: 10 });
    await ctx.quit();
  } else {
    await ctx.quit();
    asked = proc.closedAt;
  }
  await watching;
  // Its last words may come after its end is noted.
  await until("its last step", () => proc.stderr.includes(LAST_STEP) || null, 1000, { every: 20 }).catch(() => {});
  const steps = traced(proc.stderr).map(({ step, at }) => ({ step, ms: at - asked }));
  const left = leftovers(proc.exe);
  return { gone: proc.exitedAt - asked, window: window && window - asked, steps, left };
}

/** A chat far longer than Twitch's test one (chat()): 120,000 messages over four hours, seconds to fetch. */
function longChat() {
  const messages = Array.from({ length: 120_000 }, (_, i) => ({
    time: i * 0.12,
    author: `viewer${(i % 30) + 1}`,
    text: `message ${i + 1}`,
  }));
  writeFileSync(path.join(dataDirs()[0], "e2e-chat.json"), JSON.stringify({ messages, fail_after: null }));
}

const listed = async (app, id) => (await app.invoke("list_vods")).find((v) => v.id === id);

/**
 * Makes VOD `id` a Twitch VOD (`platformId`) four hours long, its chat not
 * fetched (none kept), with longChat() to fetch: the app fetches it at its
 * next start (fixtures.mjs fetchChat). With the app stopped.
 */
function chatToFetch(id, platformId) {
  longChat();
  const db = new DatabaseSync(path.join(dataDirs()[0], "library.sqlite"));
  db.prepare(
    `UPDATE vods SET platform = 'twitch', platform_id = ?, url = ?, title = 'A stream with chat', channel = 'Marlowe',
       duration = 14400, source_path = NULL, chat = NULL, chat_error = NULL WHERE id = ?`,
  ).run(platformId, `https://www.twitch.tv/videos/${platformId}`, id);
  db.prepare("DELETE FROM chat WHERE vod_id = ?").run(id);
  db.close();
}

/**
 * The states, each: `setup(app, ctx)` once (returns why it can't run here,
 * or nothing), `before()` before each start, `ready(app, ctx)` after it,
 * till the state is reached (returning the app, if it started it again),
 * and `after(app)` once its quits are done, so the next state doesn't have
 * its work going on; `times` quits each way, if not TIMES.
 */
function states(dir) {
  const s = {};
  return [
    { name: "idle, empty library", short: "idle", ready: (app) => app.toLibrary() },
    {
      name: "a file open, paused",
      short: "paused",
      async setup(app) {
        if (!(await app.invoke("player_available"))) return "no screen for the app's own player here";
        [s.minute] = await app.addFiles([minute(dir)]);
      },
      async ready(app) {
        await app.toLibrary();
        await app.openPaused(s.minute.id);
      },
    },
    {
      name: "a file playing",
      short: "playing",
      async setup(app) {
        if (!(await app.invoke("player_available"))) return "no screen for the app's own player here";
        s.minute ??= (await app.addFiles([minute(dir)]))[0];
      },
      async ready(app) {
        await app.toLibrary();
        await app.openVod(s.minute.id);
        // From the start each time, so it's still playing at the quit.
        if (await app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`)) await app.pause();
        await app.toStart();
        await app.plays();
      },
    },
    {
      name: "an edit open",
      short: "edit",
      async setup(app) {
        s.minute ??= (await app.addFiles([minute(dir)]))[0];
        s.edit = await app.invoke("create_edit");
        await app.invoke("add_range_to_edit", { editId: s.edit.id, vodId: s.minute.id, start: 1, end: 9 });
      },
      async ready(app) {
        await app.toLibrary();
        if (!(await app.js(`return !!document.querySelector("section.edit-view")`)))
          await app.click(`.vod.edit[data-key="-${s.edit.id}"] button.main`);
        await until("the edit open", () => app.js(`return !!document.querySelector("section.edit-view")`), MINUTE);
      },
      after: (app) => app.invoke("delete_edit", { id: s.edit.id }),
    },
    {
      name: "a clip being made",
      short: "clip",
      async setup(app) {
        [s.long] = await app.addFiles([longVideo(dir)]);
      },
      async ready(app, ctx) {
        await app.toLibrary();
        // One left unmade by the last quit is made again at the start: that one, or a new one.
        const making = (clips) => clips.find((c) => c.status === "cutting" || c.status === "waiting");
        if (!making(await app.invoke("list_clips", { vodId: s.long.id })))
          await app.invoke("add_clip", { vodId: s.long.id, start: 0, end: 180 });
        await until(
          "the clip being made",
          async () => (await app.invoke("list_clips", { vodId: s.long.id })).some((c) => c.status === "cutting"),
          MINUTE,
          { every: 50 },
        );
        await sleep(500);
        const clips = await app.invoke("list_clips", { vodId: s.long.id });
        if (!making(clips)) ctx.log(`the clip was made before the quit: ${clips.map((c) => c.status).join(", ")}`);
      },
      async after(app) {
        const ids = (await app.invoke("list_clips", { vodId: s.long.id })).map((c) => c.id);
        await app.invoke("delete_clips", { ids });
      },
    },
    {
      name: "a transcription running",
      short: "transcribing",
      async setup(app) {
        [s.speech] = await app.addFiles([longSpeech(dir)]);
        await app.click(`.vod[data-key="${s.speech.id}"] .actions button[aria-label="Transcribe"]`);
      },
      async ready(app) {
        await app.toLibrary();
        // Carried on at each start, as after any quit.
        await until(
          "the transcription under way",
          async () => {
            const v = await listed(app, s.speech.id);
            if (v.status === "done" || v.status === "error") throw new Error(`it's ${v.status}: make the file longer`);
            return v.status === "transcribing";
          },
          5 * MINUTE,
        );
        // A few seconds in, so the worker is at work (as quit-mid-job).
        await sleep(3000);
      },
      after: (app) => app.invoke("cancel_vod", { id: s.speech.id }),
    },
    {
      name: "a chat being fetched",
      short: "chat",
      async setup(app, ctx) {
        [s.chat] = await app.addFiles([videos(dir).video]);
        await ctx.quit();
      },
      before: () => chatToFetch(s.chat.id, TWITCH_VOD),
      async ready(app, ctx) {
        await app.toLibrary();
        await until("the chat being fetched", async () => (await listed(app, s.chat.id))?.chat === "pending", MINUTE, {
          every: 50,
        });
        await sleep(300);
        const v = await listed(app, s.chat.id);
        if (v.chat !== "pending") ctx.log(`the chat was ${v.chat} before the quit`);
      },
      // Fetched whole, so the next state's starts don't fetch it.
      after: (app) =>
        until("the chat fetched", async () => (await listed(app, s.chat.id))?.chat === "done", 3 * MINUTE),
    },
    {
      // Removed from the library (its files kept), a VOD is erased as the
      // app quits, for three seconds at most (vods.rs, SWEEP_AT_QUIT): here
      // one with a long chat, each time a new one, its chat fetched whole.
      name: "a VOD with a long chat just removed",
      short: "sweep",
      times: 3,
      setup() {
        s.sweeps = 0;
      },
      async ready(app, ctx) {
        await app.toLibrary();
        const file = path.join(dir, `removed-${++s.sweeps}.mp4`);
        copyFileSync(videos(dir).video, file);
        const [vod] = await app.addFiles([file]);
        await ctx.quit();
        chatToFetch(vod.id, `${TWITCH_VOD}${s.sweeps}`);
        app = await ctx.start();
        await app.toLibrary();
        await until("its chat fetched", async () => (await listed(app, vod.id))?.chat === "done", 3 * MINUTE);
        await app.invoke("delete_vod", { id: vod.id });
        return app;
      },
    },
  ];
}

export default {
  name: "perf-exit",
  before() {
    process.env.VODSCRIBE_TRACE_EXIT = "1";
  },
  async run(first, ctx) {
    try {
      await timeQuits(first, ctx);
    } finally {
      delete process.env.VODSCRIBE_TRACE_EXIT;
    }
  },
};

async function timeQuits(first, ctx) {
  if (first.driver.proc.stderr === undefined)
    ctx.skip("needs --driver bridge: under WebDriver the driver ends the app from outside");
  const cannot = os.unavailable();
  if (cannot) ctx.skip(cannot);
  const only = process.env.E2E_EXIT_STATES?.split(",");
  const measures = new Measures(ctx, "perf-exit");
  let app = first;
  const running = async () => (app ??= await ctx.start());

  // Ctrl+Q, as many apps quit: said either way.
  await app.toLibrary();
  os.activate(app.driver.proc);
  execFileSync("xdotool", ["key", "ctrl+q"]);
  await sleep(2000);
  if (app.driver.proc.exitedAt !== null) {
    ctx.log("Ctrl+Q quits the app");
    await ctx.quit();
    app = null;
  } else ctx.log("Ctrl+Q: no quit (the app still runs 2 s after)");

  const summary = [];
  for (const state of states(path.join(ctx.out, "fixtures"))) {
    if (only && !only.includes(state.short)) continue;
    const why = await state.setup?.(await running(), ctx);
    // A setup that quit the app (to change its library) leaves it to be started.
    if (app.driver.proc.exitedAt !== null) app = null;
    if (why) {
      ctx.log(`${state.name}: not timed (${why})`);
      continue;
    }
    for (const way of ["close", "quit"]) {
      const samples = [];
      for (let i = 0; i < (state.times ?? TIMES); i++) {
        if (!app) {
          state.before?.();
          app = await ctx.start();
        }
        const started = await state.ready(app, ctx);
        if (started instanceof App) app = started;
        const from = ctx.clock();
        const sample = await quitBy(way, app, ctx);
        app = null;
        const step = `${state.short} (${way}): process gone`;
        measures.add(step, sample.gone, from, ctx.clock());
        if (sample.left.length) ctx.log(`still running after the quit:\n  ${sample.left.join("\n  ")}`);
        samples.push(sample);
      }
      const line = said(state.name, way, samples);
      ctx.log(line);
      summary.push(line);
    }
    if (state.after) await state.after(await running());
  }
  ctx.log(`quitting, medians (ms from the ask):\n  ${summary.join("\n  ")}`);
  measures.check();
}

/** One state and way's medians: the process gone, the window gone, and each step. */
function said(name, way, samples) {
  const m = (values) => (values.length ? Math.round(median(values)) : "?");
  const windows = samples.map((s) => s.window).filter((w) => w !== null);
  const steps = [...new Set(samples.flatMap((s) => s.steps.map((t) => t.step)))].map(
    (step) => `${step} ${m(samples.flatMap((s) => s.steps.filter((t) => t.step === step).map((t) => t.ms)))}`,
  );
  return (
    `${name}, ${way === "close" ? "window closed" : "app's quit"}: gone ${m(samples.map((s) => s.gone))} ` +
    `(${samples.map((s) => s.gone).join(", ")}), window off ${m(windows)}; ${steps.join(", ")}`
  );
}
