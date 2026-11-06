// A person's streamer card, opened at the pointer by a click on their name or
// their picture over their lines in the transcript, and by the VOD's channel
// over the player. Two Twitch VODs of Marlowe's channel, made by importing
// fixtures/annotations-minute.json with a cast of four: Marlowe (played by
// Vance), Ada, Pell and Jules. The cards are served by the run's file host
// (lib/fileHost.mjs) from fixtures/streamer-cards.json: Vance's has
// everything (a picture, three channels, aliases, a social link and a
// NoPixel section with two characters, one with its wiki page), Ada's a name
// and two channels, Pell's a name alone, and Jules has none. The first VOD's
// annotations name each card by its id and the VOD's context, NoPixel; the
// second's name neither, so Marlowe's card is found through the index by
// their Twitch account, and it has no NoPixel section. Each card shows only
// what's there: no empty heading or list, nothing drawn with no height, the
// same gap between what is there. With the file host not answering
// (offline), Pell's card is the name the VOD gives, "Pell", and once it
// answers, the card's own, "Pell Harrow". Escape closes a card.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { until } from "../lib/app.mjs";
import { serveFiles } from "../lib/fileHost.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(here, "..", "fixtures", "annotations-minute.json");
const CHAT = path.join(here, "..", "fixtures", "annotations-minute.chat.json");
const CARDS = JSON.parse(readFileSync(path.join(here, "..", "fixtures", "streamer-cards.json"), "utf8"));
const NAMED = "v100000091";
const LOOKED_UP = "v100000092";

/** Vance's picture: four pixels square, all yellow. */
const PICTURE = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAIAAAAmkwkpAAAAEUlEQVR4nGP4eoQBjhiI4wAA3EIbkanqb5MAAAAASUVORK5CYII=",
  "base64",
);

/** The file host: the cards and their index, and Vance's picture; nothing else is there. */
function host(method, at) {
  const json = (body) => ({ status: 200, headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  if (at === "/streamers.json") return json(CARDS.index);
  const card = /^\/streamers\/([a-z0-9_-]+)\.json$/.exec(at)?.[1];
  if (card && CARDS.cards[card]) return json(CARDS.cards[card]);
  if (at === "/avatar/twitch/marlowe_live") return { status: 302, headers: { Location: "/pictures/vance.png" } };
  if (at === "/pictures/vance.png") return { status: 200, headers: { "Content-Type": "image/png" }, body: PICTURE };
  return { status: 404 };
}

/** The card open, as it reads: what it says, and what in it is empty or flat. */
const READ = `
  const card = document.querySelector('[role="dialog"] .card');
  if (!card) return null;
  const text = (el) => el.textContent.replace(/\\s+/g, " ").trim();
  const img = card.querySelector("header .avatar img");
  const rects = [...card.children].map((el) => el.getBoundingClientRect());
  const box = card.closest('[role="dialog"]').getBoundingClientRect();
  return {
    settled: card.dataset.settled === "true",
    name: text(card.querySelector(".name")),
    aliases: card.querySelector(".aliases") ? text(card.querySelector(".aliases")) : null,
    links: [...card.querySelectorAll(".links li")].map((li) => text(li) + " <" + li.querySelector("a").href + ">"),
    sections: [...card.querySelectorAll("section.context")].map((s) => text(s.querySelector("h3")) + ": " +
      [...s.querySelectorAll("li")].map((li) => text(li) + (li.querySelector("a") ? " <" + li.querySelector("a").href + ">" : "")).join(", ")),
    picture: img ? (!img.complete ? "loading" : img.naturalWidth > 0 && img.classList.contains("loaded") ? "shown" : "none") : "no img",
    empty: [...card.querySelectorAll("ul, li, section, h3, a, .aliases")].filter((el) => !text(el)).map((el) => el.tagName.toLowerCase() + "." + el.className),
    flat: [...card.children, ...card.querySelectorAll("li, h3")].filter((el) => el.getBoundingClientRect().height < 1).map((el) => el.tagName.toLowerCase()),
    gaps: rects.slice(1).map((r, i) => Math.round(r.top - rects[i].bottom)),
    left: Math.round(box.left),
    top: Math.round(box.top),
    bottom: Math.round(box.bottom),
  };`;

export default {
  name: "streamer-card",
  expects: {
    "streamer-card-full":
      'A small card floating over the window: a round yellow picture beside the bold name "Vance" and "also Mar, V" under it; then, under a thin rule, links in the accent colour: "twitch.tv/marlowe_live", "youtube.com/@vanceplays", "kick.com/vance" and "Bluesky", each with a small arrow sign; then, under another rule, a small grey heading "NOPIXEL" over "Marlowe" (a link) and "Dez Kade". Nothing in it overlaps or is cut off, and there are no empty headings or blank stretches.',
    "streamer-card-name":
      'A small card floating over the window with only a round coloured disc holding the letter "P" and the bold name "Pell Harrow" beside it: no rule, no links, no headings, no empty space under the name.',
    "streamer-card-channels":
      'A small card floating over the window: a round coloured disc with the letter "A" beside the bold name "Ada", then under a thin rule two links, "kick.com/ada_plays" and "twitch.tv/ada_plays"; nothing else, no empty heading under them.',
  },
  async run(app, ctx) {
    await app.toLibrary();
    const dir = path.join(ctx.out, "streamer-card");
    mkdirSync(dir, { recursive: true });
    /** Imports the fixture as Twitch VOD `id`, its cast and contexts as `named` says; the VOD made. */
    const importAs = async (id, named) => {
      const doc = JSON.parse(readFileSync(FIXTURE, "utf8"));
      doc.source.platform = "twitch";
      doc.source.id = id;
      const cards = { Marlowe: "marlowe", Ada: "ada", Pell: "pell" };
      doc.cast = [
        ...doc.cast,
        { name: "Pell", aliases: [], role: "friend" },
        { name: "Jules", aliases: [], role: "chatter" },
      ].map((m) => (named && cards[m.name] ? { ...m, streamer: cards[m.name] } : m));
      // Pell says the line no one was named for, and Jules the last.
      doc.transcript.segments[2].speaker = "Pell";
      doc.transcript.segments[5].speaker = "Jules";
      if (named) doc.contexts = ["nopixel"];
      const file = path.join(dir, `${id}.json`);
      writeFileSync(file, JSON.stringify(doc));
      writeFileSync(path.join(dir, `${id}.chat.json`), readFileSync(CHAT));
      return (await app.invoke("import_annotations_file", { path: file })).vod;
    };
    const named = await importAs(NAMED, true);
    const lookedUp = await importAs(LOOKED_UP, false);

    /** Clicks what `part` is ("name" or "picture") of the transcript's heading for `who`, at its middle, once it's
     *  drawn; the point clicked. */
    const clickHead = (who, part) =>
      until(
        `the transcript's heading for ${who}`,
        () =>
          app.js(
            `const head = [...document.querySelectorAll("#transcript .group-head")].find((h) => h.querySelector(".speaker")?.textContent.trim() === arguments[0]);
             if (!head) return null;
             const el = head.querySelector(arguments[1] === "name" ? ".speaker" : ".avatar");
             // In view, as anything clicked is: a heading below the fold would put its card there too.
             el.scrollIntoView({ block: "center" });
             const r = el.getBoundingClientRect();
             const [x, y] = [Math.round(r.left + r.width / 2), Math.round(r.top + r.height / 2)];
             el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, clientX: x, clientY: y, detail: 1, view: window }));
             return { x, y };`,
            who,
            part,
          ),
        10_000,
      );
    /** The card, once its lookup has settled and its picture has loaded or not. */
    const settledCard = (what) =>
      until(
        `${what}'s card`,
        async () => {
          const got = await app.js(READ);
          return got?.settled && got.picture !== "loading" && got;
        },
        10_000,
      );
    const close = async () => {
      await app.key("Escape");
      await until("the card closed", async () => (await app.js(READ)) === null, 5_000);
    };

    const wrong = [];
    /** Checks a card against `want` (the fields it should read as), and that nothing in it is empty or flat. */
    const check = (what, got, want) => {
      const seen = {
        name: got.name,
        aliases: got.aliases,
        links: got.links,
        sections: got.sections,
        picture: got.picture,
      };
      if (JSON.stringify(seen) !== JSON.stringify(want)) wrong.push(`${what}: ${JSON.stringify(seen)}`);
      if (got.empty.length) wrong.push(`${what} has empty ${got.empty.join(", ")}`);
      if (got.flat.length) wrong.push(`${what} has ${got.flat.join(", ")} drawn with no height`);
      if (got.gaps.some((g) => g !== got.gaps[0])) wrong.push(`${what}'s parts are ${got.gaps.join(", ")} px apart`);
    };

    // Offline: the file host answers nobody. Pell's card is what the VOD knows.
    await app.openVod(named.id);
    await clickHead("Pell", "name");
    const offline = await settledCard("Pell, offline");
    const known = { name: "Pell", aliases: null, links: [], sections: [], picture: "no img" };
    check("Pell's card offline", offline, known);
    await close();

    const served = serveFiles(host);
    const vance = {
      name: "Vance",
      aliases: "also Mar, V",
      links: [
        "twitch.tv/marlowe_live <https://www.twitch.tv/marlowe_live>",
        "youtube.com/@vanceplays <https://www.youtube.com/@vanceplays>",
        "kick.com/vance <https://kick.com/vance>",
        "Bluesky <https://bsky.app/profile/vance.example>",
      ],
      sections: ["NoPixel: Marlowe <https://nopixel.fandom.com/wiki/Marlowe>, Dez Kade"],
      picture: "shown",
    };
    const ada = {
      name: "Ada",
      aliases: null,
      links: [
        "kick.com/ada_plays <https://kick.com/ada_plays>",
        "twitch.tv/ada_plays <https://www.twitch.tv/ada_plays>",
      ],
      sections: [],
      picture: "none",
    };
    const pell = { name: "Pell Harrow", aliases: null, links: [], sections: [], picture: "no img" };
    const jules = { name: "Jules", aliases: null, links: [], sections: [], picture: "no img" };
    const people = [
      ["Marlowe (Vance)", vance, "streamer-card-full"],
      ["Ada", ada, "streamer-card-channels"],
      ["Pell", pell, "streamer-card-name"],
      ["Jules", jules, null],
    ];
    for (const [who, want, checkpoint] of people)
      for (const part of ["name", "picture"]) {
        const at = await clickHead(who, part);
        const got = await settledCard(`${who} (${part})`).catch(async () => app.js(READ));
        if (!got) {
          wrong.push(`no card for ${who} from their ${part}`);
          continue;
        }
        check(`${who}'s card from their ${part}`, got, want);
        // At the pointer: its left edge there, just below it, or above it on a short window.
        const below = got.top >= at.y && got.top <= at.y + 12;
        const above = got.bottom <= at.y && got.bottom >= at.y - 12;
        if (Math.abs(got.left - at.x) > 1 || !(below || above))
          wrong.push(`${who}'s card from their ${part} is at ${got.left},${got.top}, clicked at ${at.x},${at.y}`);
        if (part === "name" && checkpoint) await ctx.checkpoint(checkpoint);
        await close();
      }

    // The VOD's channel over the player: Vance's card, NoPixel and all.
    await app.click(".info .meta .channel");
    check("the channel's card", await settledCard("the channel"), vance);
    await close();

    // The VOD whose annotations name no card: Marlowe's by their account, through
    // the index, and no NoPixel section, the VOD naming no context.
    await app.openVod(lookedUp.id);
    const plain = { ...vance, sections: [] };
    for (const part of ["name", "picture"]) {
      await clickHead("Marlowe (Vance)", part);
      check(`in a VOD naming no card, Marlowe's from their ${part}`, await settledCard("Marlowe"), plain);
      await close();
    }
    await app.click(".info .meta .channel");
    check("in a VOD naming no card, the channel's", await settledCard("the channel"), plain);
    await close();
    // Ada's by her account too; Pell, with neither an id nor an account, is the name the VOD gives.
    await clickHead("Ada", "name");
    check("in a VOD naming no card, Ada's", await settledCard("Ada"), ada);
    await close();
    await clickHead("Pell", "name");
    check("in a VOD naming no card, Pell's", await settledCard("Pell"), known);
    await close();

    // Each card asked for once this session, and the index once.
    const asked = served.requests.map((r) => r.path).filter((p) => p.startsWith("/streamers"));
    const once = ["/streamers/ada.json", "/streamers/marlowe.json", "/streamers/pell.json", "/streamers.json"];
    if (JSON.stringify([...asked].sort()) !== JSON.stringify(once.sort()))
      wrong.push(`the file host was asked for ${asked.join(", ")}`);
    served.stop();
    if (wrong.length) throw new Error(wrong.join("; "));
    ctx.log("each card shows what it has and nothing for what it hasn't; NoPixel only where the VOD names it");
  },
};
