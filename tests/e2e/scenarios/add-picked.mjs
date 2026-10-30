// A, with clips picked in the clip bar, asks which edit they go in, without
// a mouse to drag them (AddToEdit.svelte, as on a highlight's row): two pills
// picked, A, New edit, Timeline: both go in it, in the order picked. The
// edit's source link then opens the VOD where its first piece starts, what
// the edit plays of it lit.

import { MINUTE, until } from "../lib/app.mjs";

const TOGGLE = `const mac = /Mac/.test(navigator.platform);
  const toggle = { ctrlKey: !mac, metaKey: mac };`;

export default {
  name: "add-picked",
  async run(app, { fixtures, log }) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(fixtures.landed);
    await app.openPaused(vod.id);
    const second = await app.invoke("add_clip", { vodId: vod.id, start: 2, end: 3, shown: true });
    const first = await app.invoke("add_clip", { vodId: vod.id, start: 0, end: 1, shown: true });
    await until(
      "the two pills",
      () =>
        app.js(
          `return [arguments[0], arguments[1]].every((id) => document.querySelector('.chip[data-clip="' + id + '"]'))`,
          second.id,
          first.id,
        ),
      MINUTE,
    );

    // The second clip first, then the first: the order they go in.
    for (const clip of [second, first])
      await app.js(
        `${TOGGLE}
         document.querySelector('.chip[data-clip="' + arguments[0] + '"]')
           .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...toggle }));`,
        clip.id,
      );
    await until(
      "two pills picked",
      () => app.js(`return document.querySelectorAll(".chip.chosen").length === 2`),
      10_000,
    );
    await app.key("a");
    await until(
      "the edits to pick from",
      () => app.js(`return !!document.querySelector("[role=dialog] .pick")`),
      10_000,
    ).catch(() => {
      throw new Error("A with two clips picked asked nothing");
    });
    const choose = (text) =>
      until(
        `"${text}" to choose`,
        () =>
          app.js(
            `const el = [...document.querySelectorAll("[role=dialog] .pick button")].find((b) => b.textContent.trim() === arguments[0]);
             if (!el) return false;
             el.click();
             return true;`,
            text,
          ),
        10_000,
      );
    await choose("New edit");
    await choose("Timeline, at the end");
    const [edit, pieces] = await until(
      "both in a new edit",
      async () => {
        const [made] = await app.invoke("list_edits");
        const view = made && (await app.invoke("get_edit", { id: made.id }));
        return view?.pieces.length === 2 && [made, view.pieces];
      },
      10_000,
    );
    const spans = pieces.map((p) => `${Math.round(p.start)}-${Math.round(p.end)}`);
    if (spans.join() !== "2-3,0-1") throw new Error(`the edit has ${spans.join(", ")}, not 2-3, 0-1`);
    log(`A put the two picked clips in "${edit.title}": ${spans.join(", ")}`);

    // The edit open, its source link followed (App.svelte `openVodAt`): the
    // VOD opens, paused where the edit's first piece of it starts, with what
    // the edit plays of it lit on its timeline.
    await app.click(`.vod.edit[data-key="-${edit.id}"] button.main`);
    await until(
      "the edit open",
      () => app.js(`return !!document.querySelector("section.edit-view button.source")`),
      MINUTE,
    );
    await app.js(`document.querySelector("section.edit-view button.source").click()`);
    await until(
      "the VOD open, what the edit plays of it lit",
      async () => {
        const [edit, lit] = await app.js(
          `return [!!document.querySelector("section.edit-view"), document.querySelectorAll(".scrubber .lit.used").length]`,
        );
        if (!edit && lit === 2 && (await app.shownTime()) === 2) return true;
        throw new Error(
          `the edit view ${edit ? "still open" : "closed"}, ${lit} stretches lit, at ${await app.shownTime()} s`,
        );
      },
      MINUTE,
    );
    log("its source link opened the VOD at 2 s, both of its stretches lit");
  },
};
