// The upgrade matrix's scenario for one version (scenarios/upgrade-<version>.mjs):
// the library and settings pair that release left (fixtures/, made by
// make-matrix-library.mjs) open in this version. The checks are the same for
// each; `expect` says what differs between releases' shapes:
//
//   - the library is at this version's format, a copy of the old one kept
//     beside it (library-v<old format>.sqlite) with its four VODs;
//   - the library lists every VOD, with its transcription (the paused one's
//     stretches as that release's shape becomes: one request for all of it,
//     or, as of 0.5.2's development, a done minute and a paused rest);
//   - a word from a transcript and a chat author (`from:`, in any case)
//     are found by search: the author index of format 28 is filled from
//     the old rows;
//   - clips, marks and the edits with their pieces (and group) are there,
//     and an edit opens in the page with its pieces;
//   - the settings kept their values, and have no key bindings;
//   - the page logged no errors.

import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs, oldLibrary } from "../lib/data.mjs";
import { LANDED } from "../lib/fixtures.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));

/** The format this version's library is at: the migrations `Db::open` runs, if the source is here. */
function latestFormat() {
  try {
    const source = readFileSync(path.join(here, "..", "..", "..", "crates", "core", "src", "db.rs"), "utf8");
    return source.match(/let migrations = \[([\s\S]*?)\];/)[1].match(/\b(?:SCHEMA|MIGRATE_V\d+),/g).length;
  } catch {
    return null;
  }
}

/**
 * The live library is at this version's format (the migrations `Db::open`
 * runs, counted from the source when it's here), not still at `from`; its
 * number.
 */
export function checkFormat(version, from) {
  const live = new DatabaseSync(path.join(dataDirs()[0], "library.sqlite"), { readOnly: true });
  const format = live.prepare("PRAGMA user_version").get().user_version;
  live.close();
  const latest = latestFormat();
  if (latest ? format !== latest : format <= from)
    throw new Error(`${version}: the library is at format ${format}, not ${latest ?? `after ${from}`}`);
  return format;
}

/**
 * @param version  the release, as the fixtures name it
 * @param expect   { from: the format the library was at; bands: what the
 *                 landing edit has at its top level in the page; pieces: how
 *                 many get_edit gives it (a group's included); paused: the
 *                 Kick VOD's stretches as [start, end, status] (its last
 *                 line's end where `reached` is "last") }
 */
export function upgradeScenario(version, expect) {
  return {
    name: `upgrade-${version}`,
    before() {
      oldLibrary(path.dirname(LANDED), version, { "{FIXTURES}": path.dirname(LANDED) });
    },
    async run(app, ctx) {
      await app.toLibrary();
      const fail = (what) => {
        throw new Error(`${version}: ${what}`);
      };
      const same = (what, got, want) => {
        if (JSON.stringify(got) !== JSON.stringify(want))
          fail(`${what} is ${JSON.stringify(got)}, not ${JSON.stringify(want)}`);
      };

      // The migration ran: this version's format, the old library kept.
      const dir = dataDirs()[0];
      const format = checkFormat(version, expect.from);
      const copy = path.join(dir, `library-v${expect.from}.sqlite`);
      if (!existsSync(copy)) fail(`no copy of the old library at ${copy}`);
      const kept = new DatabaseSync(copy, { readOnly: true });
      const old = kept.prepare("SELECT count(*) AS n FROM vods").get().n;
      const oldFormat = kept.prepare("PRAGMA user_version").get().user_version;
      kept.close();
      if (old !== 4 || oldFormat !== expect.from) fail(`the kept copy has ${old} VODs at format ${oldFormat}`);

      // Every VOD, in the library's list.
      const vods = await app.invoke("list_vods");
      same(
        "the library",
        vods.map((v) => [v.platform, v.title, v.status]).sort(),
        [
          ["kick", "Walking the wetland trail", "paused"],
          ["local", "eagle-has-landed", "done"],
          ["twitch", "Speedrunning the old tutorial", "done"],
          ["youtube", "Building a lighthouse on the cliff", "done"],
        ].sort(),
      );
      const by = (platform) => vods.find((v) => v.platform === platform);
      if (vods.some((v) => !v.listed)) fail("a VOD isn't in the library's list");

      // Transcription: whole for the done ones, the paused one as this release's shape becomes.
      const stretches = async (vod) =>
        (await app.invoke("transcript_stretches", { vodId: vod.id })).map((s) => [s.start, s.end, s.status, s.reached]);
      for (const platform of ["local", "youtube", "twitch"])
        same(`the ${platform} VOD's stretches`, await stretches(by(platform)), [[0, null, "done", null]]);
      const kick = by("kick");
      const lines = await app.invoke("transcript", { vodId: kick.id });
      const last = Math.max(...lines.map((l) => l.end));
      same(
        "the paused VOD's stretches",
        await stretches(kick),
        expect.paused.map(([a, b, status, reached]) => [a, b, status, reached === "last" ? last : reached]),
      );
      if (kick.transcribed !== "some") fail(`the paused VOD is transcribed ${kick.transcribed}`);

      // Search: a word of the transcript, and a chat author in any case (format 28's index).
      const speech = (await app.invoke("search", { query: "lighthouse", filter: {} })).hits;
      if (!speech.length || speech.some((h) => h.source !== "speech" && h.source !== "chat"))
        fail(`"lighthouse" finds ${JSON.stringify(speech.map((h) => [h.vod_id, h.source]))}`);
      if (!speech.some((h) => h.vod_id === by("youtube").id && h.source === "speech"))
        fail('"lighthouse" isn\'t found in what was said');
      for (const typed of ["from:nightowl42", "from:NightOwl42 night"]) {
        const found = (await app.invoke("search", { query: typed, filter: {} })).hits;
        const wanted = typed.endsWith("night") ? 1 : 4;
        if (found.length !== wanted || found.some((h) => h.source !== "chat" || h.author !== "NightOwl42"))
          fail(
            `${typed} finds ${JSON.stringify(found.map((h) => [h.vod_id, h.source, h.author]))}, not ${wanted} of NightOwl42's`,
          );
      }
      const gift = (await app.invoke("search", { query: "in:chat stone", filter: {} })).hits;
      if (gift.length !== 1 || gift[0].author !== "LoudLarry") fail(`chat for "stone" finds ${gift.length}`);

      // Clips, marks.
      same(
        "the clips on the bars",
        await Promise.all(
          vods.map(async (v) => [v.platform, (await app.invoke("list_clips", { vodId: v.id })).length]),
        ),
        vods.map((v) => [v.platform, { local: 1, youtube: 2, twitch: 1, kick: 0 }[v.platform]]),
      );
      same(
        "the marks on the YouTube VOD",
        (await app.invoke("list_marks", { vodId: by("youtube").id })).map((m) => [m.kind, m.start]).sort(),
        [
          ["chapter", 0],
          ["cut", 24],
          ["highlight", 40],
        ],
      );

      // Edits with their pieces.
      const edits = await app.invoke("list_edits");
      same("the edits", edits.map((e) => e.title).sort(), ["Landing highlights", "Stream highlights"]);
      const landing = edits.find((e) => e.title === "Landing highlights");
      const stream = edits.find((e) => e.title === "Stream highlights");
      const landed = await app.invoke("get_edit", { id: landing.id });
      const streamed = await app.invoke("get_edit", { id: stream.id });
      if (landed.pieces.length !== expect.pieces) fail(`the landing edit has ${landed.pieces.length} pieces`);
      same(
        "the stream edit's VODs",
        streamed.pieces.map((p) => p.vod_id),
        [by("youtube").id, by("twitch").id, kick.id],
      );
      same(
        "the stretches struck from the stream edit",
        streamed.pieces.map((p) => p.struck.length),
        [0, 1, 0],
      );
      same(
        "the stretches struck from the landing edit",
        landed.pieces.slice(0, 3).map((p) => p.struck.length),
        [0, 1, 0],
      );
      const groups = landed.pieces.filter((p) => p.group);
      if (groups.length !== expect.pieces - 3) fail(`the landing edit's group has ${groups.length} pieces`);

      // The edit opens in the page with its pieces.
      await app.click(`.vod.edit[data-key="-${landing.id}"] button.main`);
      const title = 'section.edit-view input.title[aria-label="Edit name"]';
      await until("the edit open", () => app.js(`return !!document.querySelector(arguments[0])`, title), MINUTE);
      const shown = await app.js(`return document.querySelector(arguments[0]).value`, title);
      if (shown !== "Landing highlights") fail(`the edit opened is named "${shown}"`);
      await until(
        "its pieces",
        async () =>
          (await app.js(`return document.querySelectorAll("section.edit-view .piece").length`)) === expect.bands,
        10_000,
      );
      await ctx.checkpoint(`upgraded-${version}`);

      // The settings kept: the choices made, the order of the emote providers, no key bindings.
      const settings = await app.invoke("get_settings");
      same("the settings", [settings.theme, settings.clip_format, settings.video_height], ["light", "mov", 720]);
      same("the emote providers", settings.emote_providers, [
        { id: "ffz", on: false },
        { id: "7tv", on: false },
        { id: "bttv", on: false },
      ]);
      same("the key bindings", settings.key_bindings, {});

      // Nothing logged to the page's console as an error.
      const logged = await app.js(`return window.__e2e?.console ?? []`);
      if (logged.length) fail(`the page logged errors: ${logged.join(" | ")}`);
      app.log(
        `${version}: format ${expect.from} to ${format}; ${vods.length} VODs, search, clips, edits and settings kept`,
      );
    },
  };
}
