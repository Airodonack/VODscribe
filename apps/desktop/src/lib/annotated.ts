// What an imported annotations file (annotations.ts) shows, worked out for
// the views: the colours its cast and activity are drawn in, and what's said
// of them (a highlight's kind is an emoji, annotations.ts `highlightGlyph`;
// chapters have no colour of their own). The data comes through the app's
// commands (list_marks, transcript, vod_annotations), never from the file.

import type { Activity, CastMember } from "./api";

/** The cast's colours, by their place in the cast: each reads on the light
 *  and the dark theme alike, and none is the accent. */
export const CAST_COLORS = ["#2f7de1", "#d9534f", "#1f9d6b", "#c27c0e", "#8a5cd6", "#0e9aa7", "#c2408a", "#6b8e23"];

/** What the VOD is doing, by kind; another kind is gray. */
export const ACTIVITY_COLORS: Record<string, string> = {
  game: "#2f7de1",
  "just chatting": "#1f9d6b",
  event: "#db2777",
  break: "#8c8c86",
};

const OTHER = "#8c8c86";

/** The place in `cast` of whoever `name` is: by their name or one of their aliases, any case; -1 if none. */
export function castIndex(cast: CastMember[], name: string | null | undefined): number {
  const key = name?.trim().toLowerCase();
  if (!key) return -1;
  return cast.findIndex((m) => m.name.toLowerCase() === key || m.aliases.some((a) => a.toLowerCase() === key));
}

/** The colour of `name`'s lines, by their place in the cast; null for someone not in it. */
export function speakerColor(cast: CastMember[], name: string | null | undefined): string | null {
  const i = castIndex(cast, name);
  return i < 0 ? null : CAST_COLORS[i % CAST_COLORS.length];
}

/** An activity's colour, by its kind. */
export const activityColor = (kind: string): string => ACTIVITY_COLORS[kind.toLowerCase()] ?? OTHER;

/** Words with their first letter a capital: "just chatting" → "Just chatting". */
const sentence = (s: string) => (s ? s[0].toUpperCase() + s.slice(1) : s);

/** What an activity says on the timeline: "Game: Chess", "Just chatting". */
export function activityLabel(a: Activity): string {
  const name = a.name?.trim();
  return name ? `${sentence(a.kind)}: ${name}` : sentence(a.kind);
}

/** A cast member's role and aliases, as the cast list says them: "Streamer · also Mar, M". An alias
 *  that is their name, or their player's (already in the name they're listed by, castNames.ts), isn't repeated. */
export function castDetail(m: CastMember): string {
  const said = [m.name, m.player ?? ""].map((n) => n.trim().toLowerCase());
  const aliases = m.aliases.filter((a) => a.trim() && !said.includes(a.trim().toLowerCase()));
  return [sentence(m.role), aliases.length ? `also ${aliases.join(", ")}` : ""].filter(Boolean).join(" · ");
}

/**
 * Highlights close together on a timeline, as one glyph each: walking them
 * in order of `x` (where each is drawn, in pixels), one less than `within`
 * from the first of the group before joins it. The first of each group is
 * the one drawn (and what a click goes to); `count` says how many it stands
 * for. Zoomed in, they come apart.
 */
export function crowds<T>(items: T[], x: (item: T) => number, within: number): { first: T; count: number }[] {
  const sorted = items.map((item) => ({ item, at: x(item) })).sort((a, b) => a.at - b.at);
  const out: { first: T; count: number; at: number }[] = [];
  for (const { item, at } of sorted) {
    const last = out.at(-1);
    if (last && at - last.at < within) last.count++;
    else out.push({ first: item, count: 1, at });
  }
  return out.map(({ first, count }) => ({ first, count }));
}
