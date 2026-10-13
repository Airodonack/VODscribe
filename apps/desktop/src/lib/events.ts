// Chat events (subs, gifts, tips): built-in timeline toggles, named as each
// site names them. Together they're one more layer, beside the term layers,
// in the platform's color: each kind at its own strength.

import type { ChatKind, ChatLine } from "./api";

export type EventKind = Exclude<ChatKind, "message">;

export const EVENTS: { kind: EventKind; icon: "star" | "gift" | "gem"; alpha: number }[] = [
  { kind: "sub", icon: "star", alpha: 1 },
  { kind: "gift", icon: "gift", alpha: 0.62 },
  { kind: "tip", icon: "gem", alpha: 0.34 },
];

/** Each platform's color, as in app.css (--youtube, --twitch, --kick). */
const PLATFORM_COLORS: Record<string, string> = { youtube: "#ff0033", twitch: "#9146ff", kick: "#53fc18" };

/** An event kind's color on `platform`'s VODs, as `rgb(… / alpha)`. */
export function eventColor(kind: EventKind, platform: string): string {
  const hex = PLATFORM_COLORS[platform] ?? "#888888";
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  const alpha = EVENTS.find((e) => e.kind === kind)?.alpha ?? 1;
  return `rgb(${r} ${g} ${b} / ${alpha})`;
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
