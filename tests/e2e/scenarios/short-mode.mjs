// Short mode: in a window too short for the player and the transcript
// together (a 1024 by 768 Mac's 613 px page), a VOD opens with its player
// alone in the main column, its picture larger than usual, the timeline and
// bar under it at the window's bottom, and no transcript; the header's
// button reads "Show transcript and chat". Pressed, the transcript takes the
// column (its minimap the height of it) and the button reads "Show player".
// Another VOD opened keeps the transcript in the player's place; "Show
// player" brings that VOD's player back alone. (The other scenarios run
// with short mode off: lib/app.mjs shortMode.)

import path from "node:path";

import { MINUTE, resizeInner, until } from "../lib/app.mjs";
import { videos } from "../lib/fixtures.mjs";

const TOGGLE = ".app > .main > section .head button[aria-expanded]";

/** What the main column shows: each part's box (null when it isn't drawn), and the toggle's label. */
const layout = (app) =>
  app.js(
    `
    const main = document.querySelector(".app > .main");
    const box = (selector) => {
      const el = main.querySelector(selector);
      if (!el || !el.getClientRects().length) return null;
      const r = el.getBoundingClientRect();
      return { top: Math.round(r.top), bottom: Math.round(r.bottom), height: Math.round(r.height) };
    };
    return {
      page: innerHeight,
      alone: main.classList.contains("alone"),
      panel: box(":scope > section"),
      stage: box(".stage"),
      controls: box(".controls"),
      below: box(".below"),
      minimap: box(".minimap"),
      label: main.querySelector(arguments[0])?.getAttribute("aria-label") ?? null,
    };`,
    TOGGLE,
  );

/** Waits for the player alone in the column, filling it; returns its layout. */
async function playerAlone(app, what) {
  let last;
  try {
    return await until(
      what,
      async () => {
        last = await layout(app);
        const { page, alone, panel, stage, controls, below, label } = last;
        return (
          alone &&
          !below &&
          stage &&
          controls &&
          // The panel down to the window's bottom (its 1 px border), the
          // picture larger than the usual 44% of the window.
          panel.bottom >= page - 1 &&
          stage.height > page * 0.44 + 20 &&
          label === "Show transcript and chat" &&
          last
        );
      },
      10_000,
    );
  } catch (e) {
    throw new Error(`${e.message}: ${JSON.stringify(last)}`);
  }
}

/** Waits for the transcript in the player's place, taking the column; returns its layout. */
async function transcriptShown(app, what, { minimap }) {
  let last;
  try {
    return await until(
      what,
      async () => {
        last = await layout(app);
        const { page, alone, panel, stage, controls, below, label } = last;
        return (
          !alone &&
          !stage &&
          !controls &&
          below &&
          // All under the toolbar and the player's header.
          below.top <= panel.bottom + 1 &&
          below.bottom >= page - 1 &&
          (!minimap || last.minimap?.height > below.height / 2) &&
          label === "Show player" &&
          last
        );
      },
      10_000,
    );
  } catch (e) {
    throw new Error(`${e.message}: ${JSON.stringify(last)}`);
  }
}

export default {
  name: "short-mode",
  expects: {
    "short-player-alone":
      "Under the search bar, a video's title, then a large test picture (coloured bars) filling most of the window, the timeline and a bar of buttons (Split, Clip, Transcribe) at the window's bottom; no transcript anywhere under them.",
    "short-transcript":
      "Under the search bar, a video's title with a chevron button at its right, then the Transcript heading and its lines filling the rest of the window down to its bottom, with a narrow strip at the right edge; no picture and no timeline.",
  },
  async run(app, ctx) {
    await app.shortMode(true);
    const size = await resizeInner(app, 1200, 613);
    ctx.log(`the page ${size.width}x${size.height}`);
    const { video, named } = videos(path.join(ctx.out, "fixtures"));
    await app.toLibrary();
    const first = await app.addAndTranscribe(video);
    const [second] = await app.addFiles([named]);
    await app.openVod(first.id);
    await until("its transcript", () => app.js(`return !!document.querySelector("#transcript .row")`), MINUTE);

    let seen = await playerAlone(app, "the player alone");
    ctx.log(
      `the player alone: its picture ${seen.stage.height} px of the page's ${seen.page}, the controls to ${seen.controls.bottom}, no transcript`,
    );
    await ctx.checkpoint("short-player-alone");

    await app.click(TOGGLE);
    seen = await transcriptShown(app, "the transcript in the player's place", { minimap: true });
    ctx.log(
      `"Show transcript and chat": the transcript ${seen.below.height} px from ${seen.below.top}, its minimap ${seen.minimap.height} px`,
    );
    await ctx.checkpoint("short-transcript");

    // Another VOD: the transcript stays in the player's place.
    await app.click(`.vod[data-key="${second.id}"]`);
    await until(
      "the other VOD open",
      () => app.js(`return !!document.querySelector('.vod.selected[data-key="${second.id}"]')`),
      MINUTE,
    );
    seen = await transcriptShown(app, "the transcript still in the player's place", { minimap: false });
    ctx.log(`another VOD opened: still the transcript, ${seen.below.height} px`);

    await app.click(TOGGLE);
    seen = await playerAlone(app, "its player alone");
    ctx.log(`"Show player": its player alone, its picture ${seen.stage.height} px`);
  },
};
