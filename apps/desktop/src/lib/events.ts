// Chat events (subs, gifts, tips): built-in timeline toggles, named as each
// site names them. Together they're one more layer, beside the term layers,
// in the platform's color, each moment as strong as the money it saw.

import type { ChatKind, ChatLine } from "./api";

export type EventKind = Exclude<ChatKind, "message">;

export const EVENTS: { kind: EventKind; icon: "star" | "gift" | "gem" }[] = [
  { kind: "sub", icon: "star" },
  { kind: "gift", icon: "gift" },
  { kind: "tip", icon: "gem" },
];

/** Each platform's color, as in app.css (--youtube, --twitch, --kick). */
const PLATFORM_COLORS: Record<string, string> = { youtube: "#ff0033", twitch: "#9146ff", kick: "#53fc18" };

/** The events' color on `platform`'s VODs. */
export const eventColor = (platform: string): string => PLATFORM_COLORS[platform] ?? "#888888";

/** The same as an "r g b" triplet, set beside it as `--event-rgb` for the
 *  fallbacks colorMix.js writes where there's no color-mix. */
export const eventRgb = (platform: string): string => {
  const hex = eventColor(platform);
  return [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(" ");
};

/** A sub's price by tier, in US dollars (Twitch's; Kick subs and YouTube memberships are taken as tier 1). */
const SUB_USD = [5, 5, 10, 25];
/** Bits and KICKs are each about a cent. */
const CENT = 0.01;

/**
 * Roughly how many US dollars a currency unit is worth, for weighing Super
 * Chats against each other and against subs. Fixed rates: it's for how strongly
 * a moment shows on the timeline, not for accounting. Prefixes as YouTube writes them.
 */
const USD_PER: Record<string, number> = {
  $: 1,
  US$: 1,
  CA$: 0.73,
  A$: 0.65,
  NZ$: 0.6,
  HK$: 0.13,
  NT$: 0.031,
  MX$: 0.055,
  R$: 0.18,
  "€": 1.08,
  "£": 1.27,
  "¥": 0.0067,
  "CN¥": 0.14,
  "₩": 0.00073,
  "₹": 0.012,
  "₱": 0.017,
  "₫": 0.00004,
  "₺": 0.03,
  "₪": 0.27,
  "₴": 0.024,
  "₽": 0.011,
  CHF: 1.13,
  SEK: 0.095,
  NOK: 0.093,
  DKK: 0.145,
  PLN: 0.25,
  CZK: 0.043,
  HUF: 0.0027,
  ZAR: 0.055,
  SGD: 0.74,
  MYR: 0.21,
  THB: 0.028,
  IDR: 0.000063,
  ARS: 0.001,
  CLP: 0.0011,
  COP: 0.00025,
  PEN: 0.27,
};

/**
 * A Super Chat's amount, from the start of its text ("$5.00 …", "¥500 …",
 * "€10,00 …", "₩10,000 …"), in US dollars. The number stops where it ends: at
 * the first space or separator not followed by a group of three digits, so a
 * message starting with a number ("$5.00 100 subs!") isn't read as part of it.
 */
export function superChatUsd(text: string): number | null {
  const m = /^\s*([^\d\s]*)[\s\u00a0]?(\d{1,3}(?:[.,\s\u00a0]\d{3})*(?:[.,]\d{1,2})?|\d+(?:[.,]\d{1,2})?)(?!\d)/.exec(
    text,
  );
  if (!m) return null;
  const rate = USD_PER[m[1].trim()];
  if (rate === undefined) return null;
  let n = m[2].replace(/[\s\u00a0]/g, "");
  // The last separator followed by one or two digits is the decimal point; others group thousands.
  const dec = /[.,](\d{1,2})$/.exec(n);
  n = dec ? n.slice(0, dec.index).replace(/[.,]/g, "") + "." + dec[1] : n.replace(/[.,]/g, "");
  const value = Number(n);
  return Number.isFinite(value) ? value * rate : null;
}

/** About how much an event was worth, in US dollars; 0 for a plain message. */
export function eventUsd(message: ChatLine, platform: string): number {
  switch (message.kind) {
    case "sub":
      return SUB_USD[platform === "twitch" ? Math.min(3, Math.max(1, message.amount ?? 1)) : 1];
    case "gift":
      return (message.amount ?? 1) * SUB_USD[1];
    case "tip": {
      if (platform !== "youtube") return (message.amount ?? 0) * CENT;
      const text = message.parts.map((p) => (p.type === "text" ? p.text : p.name)).join("");
      return superChatUsd(text) ?? SUB_USD[1];
    }
    default:
      return 0;
  }
}

/** The toggle's name on `platform`'s VODs. */
export function eventName(kind: EventKind, platform: string): string {
  const youtube = platform === "youtube";
  if (kind === "sub") return youtube ? "Members" : "Subs";
  if (kind === "gift") return youtube ? "Gifted" : "Gifts";
  return youtube ? "Super Chats" : platform === "kick" ? "KICKs" : "Bits";
}

/** The badge on an event message: "Sub", "Gifted 5", "1,000 KICKs", "Super Chat". */
export function eventBadge(message: ChatLine, platform: string): string | null {
  const n = message.amount?.toLocaleString();
  const youtube = platform === "youtube";
  switch (message.kind) {
    case "sub":
      return youtube ? "Member" : "Sub";
    case "gift":
      return n ? `Gifted ${n}` : "Gift";
    case "tip":
      return youtube ? "Super Chat" : n ? `${n} ${platform === "kick" ? "KICKs" : "bits"}` : "Tip";
    default:
      return null;
  }
}
