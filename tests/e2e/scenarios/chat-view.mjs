// A fetched chat as the page shows it, against a synthetic one whose bursts,
// events and words were placed (lib/fixtures.mjs chat(), served by the e2e
// build in place of Twitch's: src-tauri/src/e2e.rs; about 3,000 messages
// over two hours): the header's count is the chat's; the minimap's histogram
// has its three bursts where they were placed (asked of the app, and read
// back from the pixels the minimap drew); the chat column, scrolled to a
// moment by the minimap, shows the messages of that moment, each as it was
// written; the subs, gifts and tips are counted and badged; and a Timeline
// layer for a word counts its messages, highlights them in the column and
// draws them on the minimap in its colour, in the bursts that have the word
// and nowhere else. (The VOD has no player here, a Twitch VOD with no
// network, so "the playhead" is the part of the VOD the minimap scrolled to.
// Emote images aren't tested: the fixture's chat has the names as plain
// text, as Twitch's emote fragments and 7TV's lists aren't part of the seam.)

import { MINUTE, resizeInner, until } from "../lib/app.mjs";
import { chat, fetchChat } from "../lib/fixtures.mjs";

const BINS = 120;

export default {
  name: "chat-view",
  expects: {
    "chat-burst":
      'The chat column beside the transcript shows crowd messages like "viewer23 KEKW KEKW", one under another, with no amber highlight; the strip at the right edge is gray, with a box marking the part in view; the Timeline button above has one small gray dot.',
    "chat-layer":
      'The chat column shows messages like "viewer7 KEKW KEKW" highlighted in amber, and the minimap strip at the right edge shows amber marks where those messages are, apart from its gray spikes.',
  },
  async run(app, ctx) {
    // The chat fixture is served by the app's e2e build only (CI's Linux job
    // tests the release package).
    if (!(await app.hasE2e())) return ctx.skip("this build has no e2e feature, so the chat fixture isn't served");
    const fixture = chat();
    const { duration } = fixture;
    await app.toLibrary();
    // Layers are kept in the app's settings, which a test's reset leaves: a run
    // on the same folders as an earlier one would count its layer too.
    await app.invoke("set_timeline", { timeline: { layers: [], events: [] } });
    const added = await app.addAndTranscribe(ctx.fixtures.landed);
    ({ app } = await fetchChat(app, ctx, added.id, fixture));
    const wrong = [];
    const expect = (what, got, want) => {
      if (got !== want) wrong.push(`${what}: ${got}, not ${want}`);
    };
    await app.openVod(added.id);

    // The header's count is the chat's.
    const total = fixture.messages.length;
    const head = await until(
      "the header to count the chat",
      async () => {
        const text = await app.js(
          `return document.querySelector(".chat-status")?.textContent.replace(/\\s+/g, " ").trim() ?? ""`,
        );
        return text.includes(total.toLocaleString("en-US")) && text;
      },
      MINUTE,
    );
    expect("the header", head, `${total.toLocaleString("en-US")} messages`);
    const overview = await app.invoke("chat_overview", { vodId: added.id, duration, bins: BINS, events: true });
    expect("the chat's count", overview.count, total);

    // The histogram: a bin a minute, each with the messages placed in it, and
    // the three bursts the three busiest.
    const perMinute = Array.from({ length: BINS }, (_, i) => fixture.between(i * 60, (i + 1) * 60).length);
    expect("the histogram", overview.histogram.join(","), perMinute.join(","));
    const busiest = overview.histogram
      .map((n, i) => [n, i])
      .sort((a, b) => b[0] - a[0])
      .slice(0, 3)
      .map(([, i]) => i)
      .sort((a, b) => a - b);
    expect("the busiest minutes", busiest.join(","), fixture.bursts.map((b) => Math.floor(b.at / 60)).join(","));

    // Subs, gifts and tips, counted and in the order of the messages.
    const kinds = (kind) => overview.events.filter((e) => e.kind === kind).length;
    expect(
      "subs, gifts, tips",
      [kinds("sub"), kinds("gift"), kinds("tip")].join(","),
      [fixture.events.sub, fixture.events.gift, fixture.events.tip].join(","),
    );
    expect("events", overview.events.length, fixture.events.sub + fixture.events.gift + fixture.events.tip);

    // The minimap's pixels: the mirrored waveform is widest in the bursts.
    // Each bar sums a stretch of the VOD that grows as the strip gets shorter
    // (a bar per 2 px of height): 1 m 43 s in a 1200x800 window's 141 px strip,
    // 4 m 27 s in a 1024x645 one's 54 px, as a small screen gives (a 1024x768
    // CI runner's). The longer the stretch, the more background chat each bar
    // has besides a burst's messages, so how many times wider a burst's bar is
    // depends on the window: 23 px over 11 at 800, 25 px over 17 at 645. What
    // doesn't is that its bar stands out from every bar away from the bursts,
    // which are all background; it does at both sizes, the second checked by
    // making the window that small.
    const widths = () =>
      app.js(`
        const canvas = document.querySelector(".minimap canvas");
        const { width, height } = canvas;
        const data = canvas.getContext("2d").getImageData(0, 0, width, height).data;
        const rows = [];
        for (let y = 0; y < height; y++) {
          let n = 0;
          for (let x = 0; x < width; x++) if (data[(y * width + x) * 4 + 3] > 40) n++;
          rows.push(n);
        }
        return rows;`);
    const minimap = async () => {
      // Drawn, and for the strip's height now (after a resize, once it has redrawn).
      const bars = await until(
        "the minimap to draw",
        async () => {
          const tall = await app.js(
            `const map = document.querySelector(".minimap");
             return Math.round(map.clientHeight * devicePixelRatio)`,
          );
          const rows = await widths();
          return rows.length === tall && rows.some((n) => n > 0) && rows;
        },
        MINUTE,
      );
      // A burst's rows, and a few either side (the bars are smoothed with their neighbours').
      const near = (burst, margin) => [
        Math.max(0, Math.floor((burst.at / duration) * bars.length) - margin),
        Math.ceil(((burst.at + 60) / duration) * bars.length) + margin,
      ];
      const away = bars.filter((_, y) =>
        fixture.bursts.every((b) => {
          const [from, to] = near(b, 6);
          return y < from || y > to;
        }),
      );
      const background = Math.max(...away);
      const size = await app.js(`return innerWidth + "x" + innerHeight`);
      const widest = fixture.bursts.map((burst) => Math.max(...bars.slice(...near(burst, 3))));
      // Clearly wider: the rows of one bar differ by a couple of pixels at its edges.
      fixture.bursts.forEach((burst, i) => {
        if (!(widest[i] >= background + 3))
          wrong.push(
            `in a ${size} window the minimap at ${burst.at} s is ${widest[i]} px wide, and ${background} px away from the bursts`,
          );
      });
      ctx.log(
        `the minimap in a ${size} window: ${bars.length} px tall, ${widest.join(", ")} px at the bursts, ${background} px at most away from them`,
      );
      return bars;
    };
    const bars = await minimap();
    {
      const before = await app.js(`return { width: innerWidth, height: innerHeight }`);
      await resizeInner(app, 1024, 645);
      await minimap();
      await resizeInner(app, before.width, before.height);
    }
    // And between bursts it isn't: the quietest stretch is below the bursts'.
    const quiet = Math.max(...bars.slice(Math.floor(0.3 * bars.length), Math.floor(0.45 * bars.length)));
    if (!(quiet < Math.max(...bars)))
      wrong.push(`the minimap is as wide at 40% of the VOD (${quiet} px) as at its widest (${Math.max(...bars)} px)`);

    // Scrolled to a moment by the minimap, the column shows that moment's messages.
    const seekTo = (time) =>
      app.js(
        `const map = document.querySelector(".minimap");
         const box = map.getBoundingClientRect();
         const y = box.top + (arguments[0] / arguments[1]) * box.height;
         map.dispatchEvent(new PointerEvent("pointerdown", { clientX: box.left + 3, clientY: y, pointerId: 7, pointerType: "mouse", bubbles: true }));
         map.dispatchEvent(new PointerEvent("pointerup", { clientX: box.left + 3, clientY: y, pointerId: 7, pointerType: "mouse", bubbles: true }));`,
        time,
        duration,
      );
    const column = () =>
      app.js(`return [...document.querySelectorAll("#transcript .chat .message")].map((m) => ({
        time: Number(m.dataset.time),
        text: m.textContent.replace(/\\s+/g, ""),
        highlighted: m.classList.contains("highlighted"),
        badge: m.querySelector(".badge")?.textContent.replace(/\\s+/g, "") ?? "" }))`);
    const written = (m) => m.author.replace(/\s+/g, "") + m.text.replace(/\s+/g, "");
    const at = async (time, window) => {
      await seekTo(time);
      return until(
        `the column at ${time} s`,
        async () => {
          const shown = await column();
          return shown.some((m) => m.time >= time && m.time < time + window) && shown;
        },
        30_000,
        { every: 250 },
      );
    };
    for (const burst of fixture.bursts) {
      const shown = await at(burst.at, burst.seconds);
      // Each message shown is one that was written then, as it was written.
      const known = new Set(fixture.messages.map((m) => `${m.time}|${written(m)}`));
      // (An event's badge comes first.)
      const strange = shown.filter((m) => !known.has(`${m.time}|${m.text.slice(m.badge.length)}`));
      if (strange.length)
        wrong.push(
          `at ${burst.at} s the column shows ${strange.length} messages not in the chat, like "${strange[0].text}" at ${strange[0].time}`,
        );
      const far = shown.filter((m) => Math.abs(m.time - burst.at) > 20 * 60);
      if (far.length)
        wrong.push(`at ${burst.at} s the column also shows ${far.length} messages from over 20 minutes away`);
      if (burst === fixture.bursts[0]) await ctx.checkpoint("chat-burst");
      const crowd = shown.filter(
        (m) => m.time >= burst.at && m.time < burst.at + burst.seconds && burst.words.some((w) => m.text.includes(w)),
      );
      if (crowd.length === 0)
        wrong.push(`at ${burst.at} s the column shows none of the burst's ${burst.words.join(" or ")}`);
    }

    // A sub is badged where it was posted.
    const firstSub = overview.events.find((e) => e.kind === "sub");
    {
      const shown = await at(Math.floor(firstSub.time), 1);
      const badged = shown.filter((m) => m.badge && m.text.includes("subscribed"));
      if (badged.length === 0) wrong.push(`the sub at ${firstSub.time} s has no badge in the column`);
    }

    // A Timeline layer for a word: its count, its highlight, its colour on the minimap.
    await app.click("button.timeline");
    await app.clickText("Add a layer");
    await app.type("input.add-term", "KEKW");
    await app.js(
      `document.querySelector("input.add-term").dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }))`,
    );
    const events = await app.js(
      `return [...document.querySelectorAll(".events .event")].map((b) => b.querySelector(".count")?.textContent.trim())`,
    );
    expect(
      "the Timeline's counts of subs, gifts and tips",
      events.join(","),
      `${fixture.events.sub},${fixture.events.gift},${fixture.events.tip}`,
    );
    const word = fixture.withWord("KEKW");
    const counted = await until(
      "the layer's count",
      async () =>
        (await app.js(`return Number(document.querySelector(".layer .count")?.textContent ?? 0)`)) === word.length,
      MINUTE,
    ).catch(() => null);
    if (counted === null)
      throw new Error(`the layer for KEKW never counted ${word.length} messages; ${wrong.join("; ")}`);
    const color = await app.js(`return getComputedStyle(document.querySelector(".layer .swatch")).backgroundColor`);
    const matches = await app.invoke("chat_matches", { vodId: added.id, terms: [["KEKW"]] });
    expect("chat_matches for KEKW", matches[0].length, word.length);
    const strayMatch = matches[0].filter(
      ([, t]) => !fixture.bursts.some((b) => b.words.includes("KEKW") && t >= b.at && t < b.at + b.seconds + 1),
    );
    if (strayMatch.length) wrong.push(`KEKW matched ${strayMatch.length} messages outside the bursts that have it`);
    await app.click("button.timeline"); // closes the panel

    {
      const first = fixture.bursts[0];
      const shown = await until(
        "highlighted messages in the first burst",
        async () => {
          const s = await at(first.at, first.seconds);
          return s.some((m) => m.highlighted) && s;
        },
        30_000,
        { every: 500 },
      );
      const bad = shown.filter((m) => m.highlighted !== /KEKW/.test(m.text));
      if (bad.length)
        wrong.push(
          `${bad.length} messages in view are highlighted wrongly, like "${bad[0].text}" (${bad[0].highlighted ? "highlighted" : "not"})`,
        );
      // The second burst has none of the word: nothing in view is highlighted.
      const second = fixture.bursts[1];
      const there = await at(second.at, second.seconds);
      const lit = there.filter((m) => m.highlighted).length;
      if (lit) wrong.push(`${lit} messages are highlighted in the burst that has no KEKW`);
    }
    // On the minimap, the layer's colour is drawn at the bursts that have the word.
    {
      const amber = await app.js(`
        const canvas = document.querySelector(".minimap canvas");
        const { width, height } = canvas;
        const data = canvas.getContext("2d").getImageData(0, 0, width, height).data;
        const rows = new Array(height).fill(0);
        for (let y = 0; y < height; y++)
          for (let x = 0; x < width; x++) {
            const i = (y * width + x) * 4;
            if (data[i + 3] > 200 && data[i] > 200 && data[i + 1] > 120 && data[i + 1] < 200 && data[i + 2] < 90) rows[y]++;
          }
        return rows;`);
      const near = (burst) => {
        const from = Math.max(0, Math.floor((burst.at / duration) * amber.length) - 3);
        const to = Math.ceil(((burst.at + 60) / duration) * amber.length) + 3;
        return amber.slice(from, to).reduce((a, b) => a + b, 0);
      };
      const [first, second, third] = fixture.bursts;
      if (near(first) === 0) wrong.push("the minimap draws no layer colour at the first burst");
      if (near(third) === 0) wrong.push("the minimap draws no layer colour at the third burst");
      if (near(second) !== 0)
        wrong.push(`the minimap draws the layer's colour (${near(second)} px) at the burst without KEKW`);
      ctx.log(`the layer ${color}: ${word.length} messages counted, drawn at the bursts that have it`);
      await at(first.at, first.seconds);
      await ctx.checkpoint("chat-layer");
    }
    if (wrong.length) throw new Error(wrong.join("; "));
    ctx.log(`the header, histogram, column, events and layer all match the fixture's ${total} messages`);
  },
};
