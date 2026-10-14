// What chat events are worth (events.ts): node --test apps/desktop/src/lib/events.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { eventUsd, superChatUsd } from "./events.ts";

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
