// Transcribing a stretch: a ten-minute file, added, opens paused and isn't
// transcribed; 2:00–3:00 asked for has lines there and nowhere else, which
// search and the captions keep to; 2:30–4:00 after it replaces the lines it
// covers rather than doubling them, and the two make one stretch, 2:00–4:00.

import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { longSpeech } from "../lib/fixtures.mjs";

/** A line starting or ending this far past a stretch's edge is outside it (word times aren't exact). */
const EDGE = 0.5;

export default {
  name: "transcribe-stretch",
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

    // TODO(step 5, transcript panel): with the VOD open after 2:00–3:00, the
    // panel shows the lines of 2:00–3:00 only, gap rows "0:00 – 2:00 · not
    // transcribed · Transcribe" and "3:00 – 10:00 …" before and after them,
    // and a gap row's Transcribe asks for just that gap.

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

    // TODO(step 5, transcript panel): the panel shows 2:00–4:00 as one run of
    // lines, with no gap row between 3:00 and 4:00, and the header says
    // nothing is transcribing.
  },
};
