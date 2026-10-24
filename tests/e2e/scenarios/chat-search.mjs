// Searching a VOD's chat (and its speech) by who wrote it: `from:` a chatter
// finds only that chatter's messages, a name with a space needs its quotes,
// `from:` the channel finds the streamer's speech and their chat (alone, every
// line of their VOD and every message they wrote), `from:a,b` is either
// name, `in:chat` alone lists chat, `-gg` leaves out messages with that word, `@Ada` is
// words to find (a mention), not a filter, and the box's suggestions name the
// channel first, then chatters by how much they wrote. The chat is a
// synthetic one (lib/fixtures.mjs chat(), served by the e2e build in place
// of Twitch's: src-tauri/src/e2e.rs), about 3,000 messages over two hours;
// the VOD is the landing's file, transcribed, made the test Twitch VOD of
// the chat's channel. Asked through the app's own commands, then once typed
// into the search box.

import { MINUTE, until } from "../lib/app.mjs";
import { CHANNEL, chat, fetchChat } from "../lib/fixtures.mjs";

const SEARCH = 'input[placeholder="Search for a phrase…"]';

export default {
  name: "chat-search",
  expects: {
    "chat-search-from":
      'The search box reads "from:Ada" with a chip for the filter under it; the results are all chat messages, each with the author "Ada" before it.',
  },
  async run(app, ctx) {
    // The chat fixture is served by the app's e2e build only (CI's Linux job
    // tests the release package).
    if (!(await app.hasE2e())) return ctx.skip("this build has no e2e feature, so the chat fixture isn't served");
    const fixture = chat();
    await app.toLibrary();
    const added = await app.addAndTranscribe(ctx.fixtures.landed);
    ({ app } = await fetchChat(app, ctx, added.id, fixture));
    const search = (query, filter = {}) => app.invoke("search", { query, filter, typed: false });
    const wrong = [];
    const expect = (what, got, want) => {
      if (got !== want) wrong.push(`${what}: ${got}, not ${want}`);
    };
    // The messages of `hits`, as "author: text".
    const said = (hits) => hits.map((h) => `${h.author}: ${h.parts.map((p) => p.text).join("")}`);
    const onlyChat = (what, r) =>
      expect(`${what}: hits from speech`, r.hits.filter((h) => h.source !== "chat").length, 0);

    // from: a chatter finds that chatter's messages and no one else's, in any case.
    for (const query of ["from:Ada", "from:ada"]) {
      const r = await search(query);
      onlyChat(query, r);
      expect(`${query}: hits`, r.hits.length, fixture.by("Ada").length);
      expect(`${query}: other authors' hits`, r.hits.filter((h) => h.author !== "Ada").length, 0);
      expect(`${query}: cut off`, r.truncated, false);
      expect(`${query}: the filter read from the text`, r.query.from, query.slice(5));
    }

    // A name with a space, in its quotes: just that chatter's.
    {
      const r = await search('from:"Some One"');
      onlyChat('from:"Some One"', r);
      expect('from:"Some One": hits', r.hits.length, fixture.by("Some One").length);
      expect('from:"Some One": other authors', r.hits.filter((h) => h.author !== "Some One").length, 0);
    }

    // from: the channel alone lists all the streamer said, every line of the
    // VOD (all its speech is the channel's), and all they wrote in chat; in:
    // narrows it to one or the other.
    {
      const lines = (await app.invoke("transcript", { vodId: added.id })).length;
      if (lines === 0) wrong.push("the landing's transcript has no lines");
      const all = await search(`from:${CHANNEL}`);
      expect(`from:${CHANNEL}: speech hits`, all.hits.filter((h) => h.source === "speech").length, lines);
      expect(
        `from:${CHANNEL}: chat hits`,
        all.hits.filter((h) => h.source === "chat").length,
        fixture.by(CHANNEL).length,
      );
      expect(
        `from:${CHANNEL}: hits by others`,
        all.hits.filter((h) => h.source === "chat" && h.author !== CHANNEL).length,
        0,
      );
      expect(`from:${CHANNEL}: cut off`, all.truncated, false);
      const times = all.hits.map((h) => h.time);
      expect(
        `from:${CHANNEL}: in time order`,
        times.every((t, i) => i === 0 || times[i - 1] <= t),
        true,
      );
      const audio = await search(`from:${CHANNEL} in:audio`);
      expect(`from:${CHANNEL} in:audio: hits`, audio.hits.length, lines);
      expect(`from:${CHANNEL} in:audio: chat hits`, audio.hits.filter((h) => h.source === "chat").length, 0);
      const written = await search(`from:${CHANNEL} in:chat`);
      onlyChat(`from:${CHANNEL} in:chat`, written);
      expect(`from:${CHANNEL} in:chat: hits`, written.hits.length, fixture.by(CHANNEL).length);
      ctx.log(`from:${CHANNEL}: ${lines} line(s) said, ${fixture.by(CHANNEL).length} message(s) written`);
    }

    // Several names, comma-separated or each typed, are any of them; the
    // filter shows them all.
    {
      const want = fixture.by("Cormac").length + fixture.by("Some One").length;
      for (const query of ['from:Cormac,"Some One"', 'from:Cormac from:"Some One"', 'from:"Some One",cormac']) {
        const r = await search(query);
        onlyChat(query, r);
        expect(`${query}: hits`, r.hits.length, want);
        expect(
          `${query}: other authors' hits`,
          r.hits.filter((h) => h.author !== "Cormac" && h.author !== "Some One").length,
          0,
        );
        expect(
          `${query}: Some One's hits`,
          r.hits.filter((h) => h.author === "Some One").length,
          fixture.by("Some One").length,
        );
      }
      expect(
        'from:Cormac,"Some One": the filter',
        (await search('from:Cormac,"Some One"')).query.from,
        "Cormac, Some One",
      );
      const both = await search(`from:Ada,${CHANNEL} in:chat`);
      expect(
        `from:Ada,${CHANNEL} in:chat: hits`,
        both.hits.length,
        fixture.by("Ada").length + fixture.by(CHANNEL).length,
      );
    }

    // With words, the channel's speech too: the landing said "landed", and so
    // does their chat.
    {
      const landed = await search(`from:${CHANNEL} landed`);
      const speech = landed.hits.filter((h) => h.source === "speech").length;
      const chatHits = landed.hits.filter((h) => h.source === "chat");
      if (speech === 0) wrong.push(`from:${CHANNEL} landed: no speech hit, though the landing says it`);
      expect(
        `from:${CHANNEL} landed: chat hits`,
        chatHits.length,
        fixture.by(CHANNEL).filter((m) => /\blanded\b/.test(m.text)).length,
      );
      // Someone else's chat saying it isn't the streamer's.
      const others = await search("from:Ada landed");
      expect(
        "from:Ada landed: speech hits (the speech is the channel's)",
        others.hits.filter((h) => h.source === "speech").length,
        0,
      );
      ctx.log(`from:${CHANNEL} landed: ${speech} speech hit(s), ${chatHits.length} chat`);
    }

    // in:chat alone lists chat only, at most 500 of it; within a minute, all of it.
    {
      const r = await search("in:chat");
      onlyChat("in:chat", r);
      expect("in:chat: hits shown", r.hits.length, 500);
      expect("in:chat: cut off", r.truncated, true);
      expect("in:chat: asks for chat alone", `${r.query.sources?.chat}/${r.query.sources?.audio}`, "true/false");
      const minute = await search("in:chat after:0:20:00 before:0:21:00");
      expect("in:chat in the burst's minute: hits", minute.hits.length, fixture.between(1200, 1260).length);
      const outside = minute.hits.filter((h) => h.time < 1200 || h.time >= 1260).length;
      expect("in:chat in the burst's minute: hits outside it", outside, 0);
    }

    // -gg leaves out the messages with that word.
    {
      const r = await search("from:Ada -gg");
      const want = fixture.by("Ada").filter((m) => !/(^|\s)gg(\s|$)/i.test(m.text));
      expect("from:Ada -gg: hits", r.hits.length, want.length);
      expect("from:Ada -gg: hits with gg", said(r.hits).filter((s) => /\bgg\b/i.test(s)).length, 0);
      const bare = await search("in:chat -gg after:0:20:00 before:0:21:00");
      expect(
        "in:chat -gg in a minute: hits",
        bare.hits.length,
        fixture.between(1200, 1260).filter((m) => !/\bgg\b/i.test(m.text)).length,
      );
    }

    // @Ada is words to find in what's written, not who wrote it: mentions of
    // Ada by anyone, none of Ada's own messages for being hers.
    {
      const r = await search("@Ada");
      const mentions = fixture.messages.filter((m) => /(^|\s)@Ada(\s|$)/.test(m.text));
      expect("@Ada: filter read from the text", r.query.from, null);
      expect("@Ada: hits", r.hits.length, mentions.length);
      expect("@Ada: hits without the mention", said(r.hits).filter((s) => !s.includes("@Ada")).length, 0);
      expect(
        "@Ada: hits by someone else",
        r.hits.some((h) => h.author !== "Ada"),
        true,
      );
      expect(
        "@Ada: hits by Ada, though she never mentions herself",
        r.hits.filter((h) => h.author === "Ada").length,
        mentions.filter((m) => m.author === "Ada").length,
      );
    }

    // The suggestions for from: name the channel first, then the chatters who
    // wrote the most.
    {
      const all = await app.invoke("author_suggestions", { prefix: "", vodId: null });
      const names = all.map((s) => s.name);
      expect("suggestions: the channel is first", names[0], CHANNEL);
      expect("suggestions: the channel is marked as one", all[0]?.streamer, true);
      expect("suggestions: the channel's messages", all[0]?.messages, fixture.by(CHANNEL).length);
      expect("suggestions: then the busiest chatters", names.slice(1, 4).join(","), "Ada,Brigid,Cormac");
      expect("suggestions: Ada's messages", all[1]?.messages, fixture.by("Ada").length);
      const some = await app.invoke("author_suggestions", { prefix: "so", vodId: null });
      expect(
        "suggestions for so",
        JSON.stringify(some.map((s) => [s.name, s.streamer, s.messages])),
        JSON.stringify([["Some One", false, fixture.by("Some One").length]]),
      );
      const mar = await app.invoke("author_suggestions", { prefix: "mar", vodId: added.id });
      expect(
        "suggestions for mar",
        JSON.stringify(mar.map((s) => [s.name, s.streamer])),
        JSON.stringify([[CHANNEL, true]]),
      );
    }
    if (wrong.length) throw new Error(wrong.join("; "));
    ctx.log("from:, in:chat, -word, @mention and the suggestions all match the fixture");

    // Typed into the box, as a person does: Ada's messages, with her name on each.
    await app.toLibrary();
    await app.type(SEARCH, "from:Ada");
    const typed = await until(
      "Ada's messages",
      async () => {
        const r = await app.js(`return {
          rows: document.querySelectorAll(".results button.row").length,
          authors: [...new Set([...document.querySelectorAll(".results button.row .chat-author")].map((a) => a.textContent.trim()))] }`);
        return r.rows > 0 && r;
      },
      MINUTE,
    );
    if (typed.authors.join() !== "Ada") throw new Error(`from:Ada shows messages by ${typed.authors.join(", ")}`);
    ctx.log(`typed from:Ada: ${typed.rows} rows, all by Ada`);
    await ctx.checkpoint("chat-search-from");

    // Two names typed: both people's messages, and the filter's summary names both.
    await app.type(SEARCH, 'from:Cormac,"Some One"');
    const two = await until(
      "Cormac's and Some One's messages",
      async () => {
        const r = await app.js(`return {
          rows: document.querySelectorAll(".results button.row").length,
          authors: [...new Set([...document.querySelectorAll(".results button.row .chat-author")].map((a) => a.textContent.trim()))].sort(),
          summary: document.querySelector("button.filter")?.textContent.replace(/\\s+/g, " ").trim() ?? "" }`);
        return r.authors.join() === "Cormac,Some One" && r;
      },
      MINUTE,
    );
    if (!two.summary.includes("from Cormac, Some One"))
      throw new Error(`the filter's summary for two names: "${two.summary}"`);
    ctx.log(`typed from:Cormac,"Some One": ${two.rows} rows; the filter says "${two.summary}"`);

    // The same in the filter's From whom, with nothing typed in the box: a
    // name typed, a comma, and another picked from the suggestions lists what
    // they wrote, as from: typed alone does. And the names typed in the box
    // show there.
    const WHOM = 'input[aria-label="From whom"]';
    const rows = `return {
      rows: document.querySelectorAll(".results button.row").length,
      authors: [...new Set([...document.querySelectorAll(".results button.row .chat-author")].map((a) => a.textContent.trim()))].sort(),
      whom: document.querySelector(${JSON.stringify(WHOM)})?.value ?? null,
      disabled: document.querySelector(${JSON.stringify(WHOM)})?.disabled ?? null,
      summary: document.querySelector(".results .summary")?.textContent.replace(/\\s+/g, " ").trim() ?? "" }`;
    await app.type(SEARCH, "");
    await app.js(`document.querySelector("button.filter").click();`);
    await until(
      "the From whom control",
      async () => app.js(`return !!document.querySelector(arguments[0]);`, WHOM),
      MINUTE,
    );
    await app.type(WHOM, "Cormac, so");
    await until(
      "Some One suggested",
      async () =>
        app.js(
          `const b = [...document.querySelectorAll("ul.names button")].find((b) => b.querySelector(".label")?.textContent.trim() === "Some One");
           if (!b) return false;
           b.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));
           return true;`,
        ),
      MINUTE,
    );
    const picked = await until(
      "Cormac's and Some One's messages, picked in the control",
      async () => {
        const r = await app.js(rows);
        return r.authors.join() === "Cormac,Some One" && r;
      },
      MINUTE,
    );
    if (picked.whom !== "Cormac, Some One") throw new Error(`From whom after the pick: "${picked.whom}"`);
    const theirs = fixture.by("Cormac").length + fixture.by("Some One").length;
    if (picked.rows !== theirs) throw new Error(`From whom alone: ${picked.rows} rows, not ${theirs}`);
    if (!picked.summary.includes("from Cormac, Some One"))
      throw new Error(`From whom alone, the results say "${picked.summary}"`);
    await app.type(SEARCH, "in:chat from:Ada,Dara");
    const mirrored = await until(
      "Ada's and Dara's messages, the control showing them",
      async () => {
        const r = await app.js(rows);
        return r.authors.join() === "Ada,Dara" && r;
      },
      MINUTE,
    );
    if (mirrored.whom !== "Ada, Dara" || mirrored.disabled !== true)
      throw new Error(`From whom with from:Ada,Dara typed: "${mirrored.whom}", disabled ${mirrored.disabled}`);
    ctx.log(
      `From whom: "${picked.whom}" picked with the box empty, ${picked.rows} rows ("${picked.summary}"); "${mirrored.whom}" shown for from:Ada,Dara`,
    );
  },
};
