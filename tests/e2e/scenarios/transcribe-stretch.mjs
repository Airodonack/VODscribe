// Transcribing a stretch: a ten-minute file, added, opens paused and isn't
// transcribed; 2:00–3:00 asked for has lines there and nowhere else, which
// search and the captions keep to; 2:30–4:00 after it replaces the lines it
// covers rather than doubling them, and the two make one stretch, 2:00–4:00.
// The transcript says what isn't transcribed: at first, with Transcribe the
// whole VOD and Transcribe from here (a menu of how much); then a row for
// each part without lines, whose Transcribe transcribes just that part, and
// which shows the request at work, with Stop. The captions button waits for
// captions, and a search with no match says what it didn't look through.
// The bar's Transcribe sits at its right before the captions button and,
// hovered, lights up what it would do: the whole VOD, then the rest; held,
// it opens the menu of how much; clicked, it does the rest, and once all is
// transcribed it asks before doing all of it again. On another file with
// nothing transcribed, T pressed twice asks for the whole VOD once, and
// hovered meanwhile, the bar's Transcribe lights nothing: nothing's left to ask.

import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { landedTwice, longSpeech } from "../lib/fixtures.mjs";

/** `seconds` as the page writes a time (platform.ts formatTime). */
function formatTime(seconds) {
  const s = Math.max(0, Math.floor(seconds));
  const [h, m, sec] = [Math.floor(s / 3600), Math.floor((s % 3600) / 60), String(s % 60).padStart(2, "0")];
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** A line starting or ending this far past a stretch's edge is outside it (word times aren't exact). */
const EDGE = 0.5;

const SEARCH = 'input[placeholder="Search for a phrase…"]';

export default {
  name: "transcribe-stretch",
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    "transcribe-lit-the-rest":
      "In the bar under the timeline a Transcribe button sits at the right, just before the captions (CC) button; on the timeline the two stretches 0:00 to 2:00 and 3:00 to the end are outlined and lit, and the stretch from 2:00 to 3:00 between them is not.",
  },
  async run(app, ctx) {
    await app.toLibrary();
    const [vod] = await app.addFiles([longSpeech(path.join(ctx.out, "fixtures"))]);

    // Added: open, paused, and not transcribed.
    await until(
      "it open",
      () =>
        app.js(
          `return !!document.querySelector('.list .vod.selected[data-key="${vod.id}"]') && !!document.querySelector("button.time")`,
        ),
      MINUTE,
    );
    if (await app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`))
      throw new Error("it plays as it's added");
    const shown = () =>
      app.js(`return document.querySelector('.vod[data-key="${vod.id}"] .status-area .status')?.textContent.trim()`);
    await until("the row to say Not transcribed", async () => (await shown()) === "Not transcribed", 10_000);
    const now = async () => (await app.invoke("list_vods")).find((v) => v.id === vod.id);
    const added = await now();
    if (added.status !== "none" || added.transcribed !== "none")
      throw new Error(`added, it's ${added.status}, transcribed ${added.transcribed}`);
    ctx.log("added: open, paused, not transcribed");

    // What the page shows: the transcript, its rows for parts not
    // transcribed (whitespace made single), its header, the captions button.
    const panel = () => app.js(`return document.querySelector("#transcript")?.innerText ?? ""`);
    const gapRows = () =>
      app.js(
        `return [...document.querySelectorAll("#transcript .row.gap")].map((r) => r.textContent.replace(/\\s+/g, " ").trim())`,
      );
    const head = () =>
      app.js(`return document.querySelector(".head .status")?.textContent.replace(/\\s+/g, " ").trim() ?? ""`);
    const notTranscribing = async () => !/Transcribing/.test(await head());
    const captionsButton = () =>
      app.js(
        `const b = document.querySelector('.bar button[aria-label="Captions"]'); return b && { disabled: b.disabled, title: b.title }`,
      );

    // The stretches lit up on the timeline (to the second), not counting an
    // edit's (.used) or what Remove would take out.
    const lit = () =>
      app.js(
        `const bar = document.querySelector(".scrubber");
         const box = bar.getBoundingClientRect();
         const time = (x) => Number(bar.dataset.viewStart) + ((x - box.left) / box.width) * Number(bar.dataset.span);
         return [...bar.querySelectorAll(".lit:not(.used):not(.removing)")].map((l) => {
           const r = l.getBoundingClientRect();
           return [Math.round(time(r.left)), Math.round(time(r.right))];
         });`,
      );
    const hover = (on) =>
      app.js(
        `document.querySelector(".bar button.transcribe").dispatchEvent(new MouseEvent(arguments[0]))`,
        on ? "mouseenter" : "mouseleave",
      );
    /** The bar's Transcribe hovered: its title starts `title`, and `want` ([start, end], ±2 s) lights up (a checkpoint); then not hovered, nothing. */
    const litWhileHovered = async (what, title, want) => {
      const said = await app.js(`return document.querySelector(".bar button.transcribe")?.title ?? null`);
      if (!said?.startsWith(title)) throw new Error(`for ${what}, the bar's Transcribe says "${said}"`);
      await hover(true);
      await until(
        `${what} lit up`,
        async () => {
          const seen = await lit();
          const near =
            seen.length === want.length &&
            seen.every(([a, b], i) => Math.abs(a - want[i][0]) <= 2 && Math.abs(b - want[i][1]) <= 2);
          if (near) return true;
          throw new Error(`lit up: ${JSON.stringify(seen)}, for ${JSON.stringify(want)}`);
        },
        10_000,
      );
      await ctx.checkpoint(`transcribe-lit-${what.replace(/ /g, "-")}`);
      await hover(false);
      await until("nothing lit up", async () => (await lit()).length === 0, 10_000);
    };

    // Nothing transcribed: said, with the two ways to start.
    await until(
      "the transcript to say it's not transcribed",
      async () => /Not transcribed yet\./.test(await panel()),
      10_000,
    );
    for (const label of ["Transcribe the whole VOD", "Transcribe from here"])
      if (
        !(await app.js(
          `return [...document.querySelectorAll("#transcript button")].some((b) => b.textContent.trim() === arguments[0])`,
          label,
        ))
      )
        throw new Error(`no ${label} in the empty transcript: ${await panel()}`);
    // From here: the bar's menu of how much, from the playhead; Escape closes it, asking nothing.
    await app.clickText("Transcribe from here");
    const menu = () =>
      app.js(
        `return [...document.querySelectorAll("p.head")].map((p) => p.textContent.trim()).find((t) => /^From /.test(t)) ?? null`,
      );
    const offered = await until("the menu of how much", menu, 10_000);
    if (offered !== "From 0:00 for:") throw new Error(`Transcribe from here opened "${offered}"`);
    await app.key("Escape");
    await until("the menu to close", async () => (await menu()) === null, 10_000);
    const untouched = await app.invoke("transcript_stretches", { vodId: vod.id });
    if (untouched.length) throw new Error(`the menu, closed, asked for ${JSON.stringify(untouched)}`);
    // Its captions button, once it plays: there, but grayed out.
    await app.click('.bar button[aria-label="Play"]');
    await until(
      "it to play",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      MINUTE,
    );
    await app.pause();
    const before = await captionsButton();
    if (!before?.disabled || before.title !== "Transcribe to get captions")
      throw new Error(`with nothing transcribed, the captions button is ${JSON.stringify(before)}`);
    ctx.log("the empty transcript offers both ways; from here opens the menu; captions grayed out");

    // The bar's Transcribe, at its right just before the captions button:
    // with nothing transcribed it would do the whole VOD, lit up on the
    // timeline while it's hovered; pressed and held, the menu of how much
    // from the playhead, which let go elsewhere closes, asking nothing.
    const place = await app.js(
      `const buttons = [...document.querySelectorAll(".bar button")];
       const at = buttons.findIndex((b) => b.matches(".transcribe"));
       const context = document.querySelector(".bar .context");
       return { at, next: buttons[at + 1]?.getAttribute("aria-label") ?? null,
                right: !!context && !!(context.compareDocumentPosition(buttons[at]) & Node.DOCUMENT_POSITION_FOLLOWING) };`,
    );
    if (place.at < 0 || place.next !== "Captions" || !place.right)
      throw new Error(`the bar's Transcribe is at ${JSON.stringify(place)}, not at the right before Captions`);
    const length = Math.round(added.duration);
    await litWhileHovered("the whole VOD", "Transcribe the whole VOD", [[0, length]]);
    await app.js(
      `document.querySelector(".bar button.transcribe").dispatchEvent(new PointerEvent("pointerdown", { button: 0, bubbles: true, pointerId: 1, isPrimary: true }))`,
    );
    const held = await until("the menu, held open", menu, 10_000);
    if (!/^From \d+:\d\d for:$/.test(held)) throw new Error(`Transcribe held opened "${held}"`);
    await app.js(
      `const r = document.querySelector("button.time").getBoundingClientRect();
       const at = { clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, button: 0, bubbles: true, pointerId: 1, isPrimary: true };
       document.elementFromPoint(at.clientX, at.clientY).dispatchEvent(new PointerEvent("pointerup", at));`,
    );
    await until("the menu to close", async () => (await menu()) === null, 10_000);
    const stillNone = await app.invoke("transcript_stretches", { vodId: vod.id });
    if (stillNone.length) throw new Error(`the held menu, let go elsewhere, asked for ${JSON.stringify(stillNone)}`);
    ctx.log(
      `the bar's Transcribe: before Captions; hovered, 0-${length} s lit; held, "${held}", let go elsewhere, closed`,
    );

    /** Asks for `start`..`end` and waits till it's done: no request left. */
    const transcribe = (start, end) =>
      app.timed(`stretch ${start}-${end} transcribed`, async () => {
        await app.invoke("transcribe_stretch", { vodId: vod.id, start, end });
        return until(
          `${start}-${end} transcribed`,
          async () => {
            const v = await now();
            if (v.status === "error") throw new Error(`transcription failed: ${v.error}`);
            return v.status === "none" && v.transcribed === "some" && v;
          },
          10 * MINUTE,
        );
      });
    const lines = () => app.invoke("transcript", { vodId: vod.id });
    const outside = (list, from, to) => list.filter((l) => l.start < from - EDGE || l.end > to + EDGE);

    // 2:00–3:00.
    await transcribe(120, 180);
    const first = await lines();
    if (first.length < 5) throw new Error(`2:00–3:00 has ${first.length} lines, for a minute of speech`);
    const stray = outside(first, 120, 180);
    if (stray.length) throw new Error(`lines outside 2:00–3:00: ${JSON.stringify(stray.slice(0, 3))}`);
    const { hits } = await app.invoke("search", {
      query: "landed",
      filter: { vodId: vod.id, speech: true, chat: false },
    });
    if (!hits.length) throw new Error(`no "landed" in 2:00–3:00: ${first.map((l) => l.text).join(" ")}`);
    const strayHits = hits.filter((h) => h.time < 120 - EDGE || h.time > 180 + EDGE);
    if (strayHits.length) throw new Error(`"landed" found outside 2:00–3:00, at ${strayHits.map((h) => h.time)}`);
    const cues = await app.invoke("captions", { vodId: vod.id });
    const at = (t) => cues.find((c) => c.start <= t && t < c.end);
    if (!at(150)) throw new Error("no caption at 2:30");
    for (const t of [60, 119, 200, 400])
      if (at(t)) throw new Error(`a caption at ${t} s, where nothing is transcribed: ${JSON.stringify(at(t))}`);
    const done = (stretches) =>
      stretches.map((s) => [s.start, s.end, s.status]).filter(([, , status]) => status === "done");
    const once = await app.invoke("transcript_stretches", { vodId: vod.id });
    if (once.length !== 1 || JSON.stringify(done(once)) !== JSON.stringify([[120, 180, "done"]]))
      throw new Error(`after 2:00–3:00 the stretches are ${JSON.stringify(once)}`);
    ctx.log(`2:00–3:00: ${first.length} lines, ${hits.length} "landed", captions only there`);

    // The transcript: those lines, with a row before and after them for
    // the parts not transcribed; the captions button now works.
    const notTranscribed = (from) => new RegExp(`^${from} – \\d+:\\d\\d · not transcribed · Transcribe$`);
    await until(
      "rows for 0:00–2:00 and 3:00 on",
      async () => {
        const rows = await gapRows();
        return (
          rows.length === 2 &&
          rows[0] === "0:00 – 2:00 · not transcribed · Transcribe" &&
          notTranscribed("3:00").test(rows[1])
        );
      },
      MINUTE,
    );
    const shownTimes = await app.js(
      `return [...document.querySelectorAll("#transcript .line button.row:not(.chapter-head)")].map((b) => Number(b.dataset.time))`,
    );
    if (shownTimes.length !== first.length || shownTimes.some((t) => t < 120 - EDGE || t > 180 + EDGE))
      throw new Error(`the transcript shows lines at ${shownTimes}, for ${first.length} in 2:00–3:00`);
    await until("the captions button to work", async () => (await captionsButton())?.disabled === false, MINUTE);
    // The bar's Transcribe would now do the rest: before 2:00 and after 3:00, lit up.
    await litWhileHovered("the rest", "Transcribe the rest", [
      [0, 120],
      [180, Math.round(added.duration)],
    ]);
    ctx.log("the bar's Transcribe, hovered: 0:00-2:00 and 3:00 on lit up, the rest");
    // A search of it alone finding nothing: only chat and those lines were looked through.
    await app.type(SEARCH, "zebra crossing");
    const none = await until(
      "the empty result",
      () => app.js(`return document.querySelector(".none")?.textContent.replace(/\\s+/g, " ").trim()`),
      MINUTE,
    );
    if (!/Only chat and the transcribed parts are searched\. Transcribe the rest/.test(none))
      throw new Error(`a search of the part-transcribed VOD, finding nothing, says "${none}"`);
    await app.type(SEARCH, "");
    await until("the transcript back", async () => (await gapRows()).length === 2, 10_000);
    ctx.log(`the transcript: ${shownTimes.length} lines between rows for 0:00–2:00 and 3:00 on; the search's note`);

    // 2:30–4:00, over half of it: its lines replace those there.
    await transcribe(150, 240);
    const second = await lines();
    const strayAfter = outside(second, 120, 240);
    if (strayAfter.length) throw new Error(`lines outside 2:00–4:00: ${JSON.stringify(strayAfter.slice(0, 3))}`);
    for (const [i, line] of second.entries()) {
      const next = second[i + 1];
      if (next && next.start < line.end - EDGE)
        throw new Error(`lines overlap, doubled: ${JSON.stringify(line)} and ${JSON.stringify(next)}`);
    }
    const kept = second.filter((l) => l.start < 150);
    if (!kept.length) throw new Error("the lines of 2:00–2:30, before the second stretch, went");
    if (!second.some((l) => l.start >= 180 + EDGE)) throw new Error("no lines after 3:00");
    const twice = await app.invoke("transcript_stretches", { vodId: vod.id });
    const covered = done(twice);
    if (twice.length !== covered.length) throw new Error(`a request left: ${JSON.stringify(twice)}`);
    // One stretch (same model), from 2:00 (or the start of the line 2:30 cut
    // through, which snapping took in) to 4:00.
    if (covered.length !== 1 || covered[0][0] !== 120 || Math.abs(covered[0][1] - 240) > EDGE)
      throw new Error(`after 2:30–4:00 the stretches are ${JSON.stringify(twice)}`);
    ctx.log(`2:30–4:00 over it: ${second.length} lines, none doubled; transcribed 2:00–4:00`);

    // The transcript: 2:00–4:00 as one run of lines, with no row between
    // 3:00 and 4:00, and nothing transcribing.
    const end = (await now()).duration;
    const afterLines = async () => {
      const rows = await gapRows();
      return (
        rows.length === 2 &&
        rows[0] === "0:00 – 2:00 · not transcribed · Transcribe" &&
        /^(3:59|4:00) – /.test(rows[1]) &&
        rows
      );
    };
    await until("rows for 0:00–2:00 and 4:00 on", afterLines, MINUTE);
    // The last lines and the job's end reach the page as separate events (the
    // lines with the last progress step, the end with the job's own status
    // once it has tidied up), so for a moment the rows are in place and the
    // header still says "Transcribing … 100%". A slow runner stretched it to
    // a check's length; it is a moment, so wait it out.
    await until("the header to stop saying Transcribing", notTranscribing, 10_000).catch(async (e) => {
      throw new Error(`${e.message}: the header says "${await head()}", with nothing asked`);
    });

    // A row's Transcribe asks for just its part, shown there while it waits or runs, with Stop.
    const clickInRow = (from, label) =>
      app.js(
        `const row = [...document.querySelectorAll("#transcript .row.gap")].find((r) => r.textContent.trim().startsWith(arguments[0]));
         const button = row && [...row.querySelectorAll("button")].find((b) => b.textContent.trim() === arguments[1]);
         if (!button) throw new Error("no " + arguments[1] + " in the row from " + arguments[0]);
         button.click();`,
        from,
        label,
      );
    const AT_WORK = "(Waiting to download|Downloading \\d+%|Waiting to transcribe|Transcribing \\d+%) · Stop";
    await clickInRow("0:00 – 2:00", "Transcribe");
    const [asked] = await until(
      "the request for 0:00–2:00",
      async () => {
        const requests = (await app.invoke("transcript_stretches", { vodId: vod.id })).filter(
          (s) => s.status !== "done",
        );
        return requests.length && requests;
      },
      10_000,
    );
    if (asked.start > EDGE || asked.end === null || Math.abs(asked.end - 120) > EDGE)
      throw new Error(`the row for 0:00–2:00 asked for ${JSON.stringify(asked)}`);
    await until(
      "0:00–2:00 transcribed",
      async () => {
        const v = await now();
        if (v.status === "error") throw new Error(`transcription failed: ${v.error}`);
        return v.status === "none" && v;
      },
      10 * MINUTE,
    );
    const third = await lines();
    const strayThird = outside(third, 0, 240);
    if (strayThird.length) throw new Error(`lines outside 0:00–4:00: ${JSON.stringify(strayThird.slice(0, 3))}`);
    if (!third.some((l) => l.start < 120)) throw new Error("no lines in 0:00–2:00 after its Transcribe");
    await until(
      "one row left, for 4:00 on",
      async () => {
        const rows = await gapRows();
        return rows.length === 1 && /^(3:59|4:00) – /.test(rows[0]);
      },
      MINUTE,
    );
    ctx.log(`the row's Transcribe: lines from ${third[0].start.toFixed(1)} s, none past 4:00`);

    // The rest, asked for from its row: the header and the row say how far; Stop ends it, keeping its lines.
    const from = (await gapRows())[0].split(" ")[0];
    await clickInRow(from, "Transcribe");
    const working = await until(
      "the rest to be transcribed, said in the header and its row",
      async () => {
        const [h, rows] = [await head(), await gapRows()];
        return (
          new RegExp(`Transcribing \\d+:\\d\\d–${formatTime(end)} · \\d+%`).test(h) &&
          rows.some((r) => new RegExp(`^\\d+:\\d\\d – ${formatTime(end)} · ${AT_WORK}$`).test(r)) && [h, rows]
        );
      },
      MINUTE,
    );
    await app.js(
      `[...document.querySelectorAll("#transcript .row.gap button")].find((b) => b.textContent.trim() === "Stop").click()`,
    );
    await until(
      "it stopped",
      async () => {
        const v = await now();
        return v.status === "none" && v;
      },
      MINUTE,
    );
    const left = await until(
      "the row back, not transcribed",
      async () => {
        const rows = await gapRows();
        return rows.length === 1 && notTranscribed("\\d+:\\d\\d").test(rows[0]) && rows[0];
      },
      MINUTE,
    );
    await until("the header to stop saying Transcribing", notTranscribing, 10_000).catch(async (e) => {
      throw new Error(`${e.message}: stopped, the header still says "${await head()}"`);
    });
    ctx.log(`the rest: "${working[0]}", "${working[1].at(-1)}"; stopped, "${left}"`);

    // The bar's Transcribe, clicked: the rest, all of it now transcribed.
    // Then it would transcribe the whole VOD again, which replaces it all:
    // it asks first, as the library row's Transcribe again does; answered
    // Cancel, nothing is asked for; answered Transcribe again, all of it is.
    await app.click(".bar button.transcribe");
    await app.timed("the rest transcribed", () =>
      until(
        "all of it transcribed",
        async () => {
          const v = await now();
          if (v.status === "error") throw new Error(`transcription failed: ${v.error}`);
          return v.status === "done" && v.transcribed === "all" && v;
        },
        10 * MINUTE,
      ),
    );
    await litWhileHovered("all of it again", "Transcribe again…", [[0, Math.round(added.duration)]]);
    const pending = async () =>
      (await app.invoke("transcript_stretches", { vodId: vod.id })).filter((s) => s.status !== "done");
    const questions = await app.asked();
    await app.answer("Cancel");
    await app.click(".bar button.transcribe");
    await until("the question", async () => (await app.asked()) === questions + 1, 10_000);
    await new Promise((r) => setTimeout(r, 500));
    if ((await pending()).length) throw new Error(`answered Cancel, it asked for ${JSON.stringify(await pending())}`);
    await app.answer("Transcribe again");
    await app.click(".bar button.transcribe");
    await until("the second question", async () => (await app.asked()) === questions + 2, 10_000);
    const again = await until(
      "all of it asked for again",
      async () => {
        const asked = await pending();
        if (asked.length === 1 && asked[0].start === 0 && asked[0].end === null) return asked[0];
        throw new Error(`asked for: ${JSON.stringify(asked)}`);
      },
      10_000,
    );
    ctx.log(`the bar's Transcribe: the rest, then all of it again only once asked (request ${again.id}, 0 to the end)`);

    // Another file, nothing of it transcribed: T, then T again while the
    // whole VOD waits (behind the one above), is one request, not two.
    const [other] = await app.addFiles([landedTwice(path.join(ctx.out, "fixtures"))]);
    await until(
      "the other open",
      () =>
        app.js(
          `return !!document.querySelector('.list .vod.selected[data-key="${other.id}"]') && !!document.querySelector(".bar button.transcribe")`,
        ),
      MINUTE,
    );
    const asks = async () =>
      (await app.invoke("transcript_stretches", { vodId: other.id })).filter((s) => s.status !== "done");
    await app.key("t");
    await until("the whole of it asked for", async () => (await asks()).length === 1, 10_000);
    await app.key("t");
    await new Promise((r) => setTimeout(r, 1000));
    const twiceT = await asks();
    if (twiceT.length !== 1 || twiceT[0].start !== 0 || twiceT[0].end !== null)
      throw new Error(`T pressed twice asked for ${JSON.stringify(twiceT)}`);
    await hover(true);
    await new Promise((r) => setTimeout(r, 1000));
    const litNow = await lit();
    await hover(false);
    if (litNow.length) throw new Error(`with all of it asked for, Transcribe lights ${JSON.stringify(litNow)}`);
    ctx.log(`T twice on a file with nothing transcribed: one request (${twiceT[0].id}), nothing lit`);
  },
};
