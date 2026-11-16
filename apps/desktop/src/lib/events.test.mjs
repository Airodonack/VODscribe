// Chat events as the timeline draws them: their toggles, names, colors, badges and worth (events.ts): node --test apps/desktop/src/lib/events.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { EVENTS, eventBadge, eventColor, eventName, eventRgb, eventUsd, superChatUsd } from "./events.ts";

/** @param {number | null} actual @param {number} expected */
const near = (actual, expected) =>
  assert.ok(actual !== null && Math.abs(actual - expected) < 0.01, `${actual} is not ${expected}`);

test("reads a Super Chat's amount in its currency", () => {
  near(superChatUsd("$5.00 love it"), 5);
  near(superChatUsd("$1,000.00 big"), 1000);
  near(superChatUsd("€10,00 hi"), 10.8);
  near(superChatUsd("CA$20.00"), 14.6);
  near(superChatUsd("₩10,000 wow"), 7.3);
  near(superChatUsd("₹1,000.00"), 12);
  near(superChatUsd("R$ 25,50"), 4.59);
  near(superChatUsd("¥500 Super Sticker"), 3.35);
  near(superChatUsd("₩10 000"), 7.3);
  assert.equal(superChatUsd("RUB 100"), null, "an unknown currency");
  assert.equal(superChatUsd("great stream"), null);
  assert.equal(superChatUsd("great $5 stream"), null, "an amount that isn't at the start");
});

test("stops at the amount, whatever the message starts with", () => {
  near(superChatUsd("$5.00 100 subs!"), 5);
  near(superChatUsd("$5.00 1st time"), 5);
  near(superChatUsd("$20.00 2 hours in"), 20);
  near(superChatUsd("€10,00 5"), 10.8);
  near(superChatUsd("$5 10 more"), 5);
});

test("values events", () => {
  /**
   * @param {import("./api").ChatKind} kind @param {number | null} amount
   * @returns {import("./api").ChatLine}
   */
  const m = (kind, amount, text = "") => ({
    id: 1,
    time: 0,
    author: "a",
    color: null,
    parts: [{ type: "text", text }],
    kind,
    amount,
  });
  assert.equal(eventUsd(m("sub", 3), "twitch"), 25, "tier 3");
  assert.equal(eventUsd(m("sub", null), "kick"), 5);
  assert.equal(eventUsd(m("gift", 50), "twitch"), 250);
  near(eventUsd(m("tip", 1000), "kick"), 10);
  near(eventUsd(m("tip", null, "$100.00 gg"), "youtube"), 100);
  assert.equal(eventUsd(m("tip", null, "RUB 100 hi"), "youtube"), 5, "unknown currency: a sub's worth");
  assert.equal(eventUsd(m("message", null, "$5"), "twitch"), 0);
});

/**
 * @param {import("./api").ChatKind} kind @param {number | null} amount
 * @returns {import("./api").ChatLine}
 */
const line = (kind, amount, text = "") => ({
  id: 1,
  time: 0,
  author: "a",
  color: null,
  parts: [{ type: "text", text }],
  kind,
  amount,
});

// "Chat events (subs, gifts, tips): built-in timeline toggles" (events.ts, EVENTS).
test("the toggles are subs, gifts and tips, never plain messages, each with its icon", () => {
  assert.deepEqual(EVENTS, [
    { kind: "sub", icon: "star" },
    { kind: "gift", icon: "gift" },
    { kind: "tip", icon: "gem" },
  ]);
});

// "Each platform's color, as in app.css" (events.ts, eventColor).
test("an event is drawn in its platform's color, grey for any other", () => {
  assert.equal(eventColor("youtube"), "#ff0033");
  assert.equal(eventColor("twitch"), "#9146ff");
  assert.equal(eventColor("kick"), "#53fc18");
  assert.equal(eventColor("elsewhere"), "#888888");
});

// "The same as an "r g b" triplet" (events.ts, eventRgb).
test("the color's triplet is its red, green and blue", () => {
  assert.equal(eventRgb("twitch"), "145 70 255");
  assert.equal(eventRgb("kick"), "83 252 24");
  assert.equal(eventRgb("elsewhere"), "136 136 136");
});

// "A sub's price by tier, in US dollars (Twitch's; Kick subs and YouTube memberships are taken as tier 1)" (events.ts, SUB_USD).
test("a sub is worth its tier's price, and Kick's subs and YouTube's memberships are tier 1", () => {
  assert.equal(eventUsd(line("sub", 1), "twitch"), 5);
  assert.equal(eventUsd(line("sub", 2), "twitch"), 10);
  assert.equal(eventUsd(line("sub", 3), "twitch"), 25);
  assert.equal(eventUsd(line("sub", null), "twitch"), 5, "no tier said");
  assert.equal(eventUsd(line("sub", 3), "kick"), 5, "Kick's subs are tier 1");
  assert.equal(eventUsd(line("sub", 3), "youtube"), 5, "YouTube's memberships are tier 1");
});

// "Bits and KICKs are each about a cent" and a gift of subs (events.ts, eventUsd).
test("a gift is worth a tier 1 sub for each one given, a tip a cent for each bit or KICK", () => {
  assert.equal(eventUsd(line("gift", 1), "twitch"), 5);
  assert.equal(eventUsd(line("gift", null), "twitch"), 5, "no count said: one");
  near(eventUsd(line("tip", 500), "twitch"), 5);
  assert.equal(eventUsd(line("tip", null), "twitch"), 0, "no amount said");
});

// "Roughly how many US dollars a currency unit is worth ... Prefixes as YouTube writes them" (events.ts, superChatUsd).
test("a Super Chat's amount is read with its decimal point and its thousands apart", () => {
  near(superChatUsd("€1.234,56 hi") ?? NaN, 1333.32);
  near(superChatUsd("$1,234.56 hi") ?? NaN, 1234.56);
  near(superChatUsd("$1 234 hi") ?? NaN, 1234);
  near(superChatUsd("  $5 hi") ?? NaN, 5);
  near(superChatUsd("$ 5") ?? NaN, 5);
  near(superChatUsd("$1,234,567.00 wow") ?? NaN, 1234567);
  near(superChatUsd("$1000 hi") ?? NaN, 1000);
  near(superChatUsd("$1000.50 hi") ?? NaN, 1000.5);
});

// "A sub's worth": a Super Chat whose amount can't be read counts as one (events.ts, eventUsd "tip").
test("a Super Chat is read from its message's parts joined", () => {
  /** @type {import("./api").ChatLine} */
  const chat = {
    ...line("tip", null),
    parts: [
      { type: "text", text: "$" },
      { type: "text", text: "20.00 " },
      { type: "emote", name: "KEKW", url: "" },
    ],
  };
  near(eventUsd(chat, "youtube"), 20);
});

// "The toggle's name on `platform`'s VODs" (events.ts, eventName).
test("the toggles are named as each site names them", () => {
  assert.deepEqual(
    ["sub", "gift", "tip"].map((k) => eventName(/** @type {any} */ (k), "twitch")),
    ["Subs", "Gifts", "Bits"],
  );
  assert.deepEqual(
    ["sub", "gift", "tip"].map((k) => eventName(/** @type {any} */ (k), "kick")),
    ["Subs", "Gifts", "KICKs"],
  );
  assert.deepEqual(
    ["sub", "gift", "tip"].map((k) => eventName(/** @type {any} */ (k), "youtube")),
    ["Members", "Gifted", "Super Chats"],
  );
});

// "The badge on an event message: "Sub", "Gifted 5", "1,000 KICKs", "Super Chat"" (events.ts, eventBadge).
test("an event message's badge says what it was", () => {
  assert.equal(eventBadge(line("sub", 3), "twitch"), "Sub");
  assert.equal(eventBadge(line("sub", null), "youtube"), "Member");
  assert.equal(eventBadge(line("gift", 5), "twitch"), "Gifted 5");
  assert.equal(eventBadge(line("gift", null), "twitch"), "Gift");
  assert.equal(eventBadge(line("tip", 1000), "kick"), `${(1000).toLocaleString()} KICKs`);
  assert.equal(eventBadge(line("tip", 100), "twitch"), "100 bits");
  assert.equal(eventBadge(line("tip", null), "twitch"), "Tip");
  assert.equal(eventBadge(line("tip", 5), "youtube"), "Super Chat");
});

test("a plain message has no badge", () => {
  assert.equal(eventBadge(line("message", null), "twitch"), null);
});
