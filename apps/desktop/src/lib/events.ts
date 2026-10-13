// Chat events (subs, gifts, tips): built-in timeline toggles with fixed colors,
// named as each site names them.

import type { ChatKind, ChatLine } from "./api";

export type EventKind = Exclude<ChatKind, "message">;

// Apart from the starting layers' colors and the palette new ones take (layers.svelte.ts).
export const EVENTS: { kind: EventKind; color: string }[] = [
  { kind: "sub", color: "#8b5cf6" },
  { kind: "gift", color: "#0ea5e9" },
  { kind: "tip", color: "#22c55e" },
];

export const eventColor = (kind: EventKind) => EVENTS.find((e) => e.kind === kind)?.color ?? "#888888";

/** The toggle's name on `platform`'s VODs. */
export function eventName(kind: EventKind, platform: string): string {
  const youtube = platform === "youtube";
  if (kind === "sub") return youtube ? "Memberships" : "Subs";
  if (kind === "gift") return youtube ? "Gifted memberships" : "Gifted subs";
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
