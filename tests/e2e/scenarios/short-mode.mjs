// Short mode: in a window too short for the player and the transcript
// together (a 1024 by 768 Mac's 613 px page), a VOD opens with its player
// alone in the main column, its picture larger than usual, the timeline and
// bar under it at the window's bottom, and no transcript; the button at the
// bar's right reads "Show transcript and chat". Pressed, the transcript takes
// the column (its minimap the height of it), the bar goes with the picture,
// and the header's button reads "Show player".
// Another VOD opened keeps the transcript in the player's place. A VOD of
// sound alone has no player to swap: no button, its bar shown over the
// transcript. Back on a video, "Show player" brings its player back alone.
// In a narrow window (the library beside it) the bar's button stays in it.
// (The other scenarios run with short mode off: lib/app.mjs shortMode.)

import path from "node:path";

import { MINUTE, resizeInner, until } from "../lib/app.mjs";
import { videos } from "../lib/fixtures.mjs";

// In the bar while the player shows, in the header while it's hidden.
const TOGGLE = ".app > .main > section button.collapse";

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
      inBar: !!main.querySelector(arguments[0])?.closest(".controls .bar"),
      toggleRight: Math.round(main.querySelector(arguments[0])?.getBoundingClientRect().right ?? 0),
      panelRight: Math.round(main.querySelector(":scope > section")?.getBoundingClientRect().right ?? 0),
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
          last.inBar &&
          // Not pushed past the column's edge by the rest of the bar.
          last.toggleRight <= last.panelRight &&
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
          !last.inBar &&
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
      "Under the search bar, a video's title, then a large test picture (coloured bars) filling most of the window, the timeline and a bar of buttons (Split, Clip, Transcribe, and a crossed-out camera just before full screen at its right end) at the window's bottom; no transcript anywhere under them.",
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
    const added = await app.addFiles([named, ctx.fixtures.landed]);
    const second = added.find((v) => !v.audio_only);
    const sound = added.find((v) => v.audio_only);
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

    // Sound alone: the player isn't alone in the column, so there's nothing
    // to swap: no button, and the swap asked for doesn't hide its bar.
    await app.openVod(sound.id);
    let last;
    try {
      seen = await until(
        "sound alone: its bar over the transcript, no swap",
        async () => {
          last = await layout(app);
          const { alone, stage, below, label } = last;
          return !alone && stage && below && below.top >= stage.bottom && label === null && last;
        },
        10_000,
      );
    } catch (e) {
      throw new Error(`${e.message}: ${JSON.stringify(last)}`);
    }
    ctx.log(`sound alone: its bar ${seen.stage.height} px over the transcript, no button`);

    // A video again: the swap is back as it was left (opened by its row: the
    // player it would wait for is the one swapped out).
    await app.click(`.vod[data-key="${second.id}"]`);
    await until(
      "the video open again",
      () => app.js(`return !!document.querySelector('.vod.selected[data-key="${second.id}"]')`),
      MINUTE,
    );
    await transcriptShown(app, "the video's transcript in the player's place again", { minimap: false });
    await app.click(TOGGLE);
    seen = await playerAlone(app, "its player alone");
    ctx.log(`"Show player": its player alone, its picture ${seen.stage.height} px`);

    // The narrowest window, the library beside the player: the bar fits its
    // column, its button inside it.
    const narrow = await resizeInner(app, 760, 613);
    seen = await playerAlone(app, "the player alone in a narrow window");
    ctx.log(
      `the page ${narrow.width}x${narrow.height}: the bar's button ends at ${seen.toggleRight}, the column at ${seen.panelRight}`,
    );
  },
};
