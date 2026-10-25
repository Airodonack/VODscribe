// What an imported annotations file (annotations.ts) shows, worked out for
// the views: the colours its cast, highlights, chapters and activity are
// drawn in, and what's said of them. The data comes through the app's
// commands (list_marks, transcript, vod_annotations), never from the file.

import type { Activity, CastMember } from "./api";

/** The cast's colours, by their place in the cast: each reads on the light
 *  and the dark theme alike, and none is the accent or a highlight's. */
export const CAST_COLORS = ["#2f7de1", "#d9534f", "#1f9d6b", "#c27c0e", "#8a5cd6", "#0e9aa7", "#c2408a", "#6b8e23"];

/** A highlight's colour by its kind; another kind has the highlight colour. */
export const HIGHLIGHT_COLORS: Record<string, string> = {
  funny: "#e0a100",
  hype: "#db2777",
  drama: "#d9534f",
  wholesome: "#1f9d6b",
  lore: "#8a5cd6",
};

/** The colour imported chapters take turns in, one after another along the timeline. */
export const CHAPTER_COLORS = ["#2f7de1", "#1f9d6b", "#c27c0e", "#8a5cd6", "#0e9aa7", "#c2408a"];

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

/** A highlight's dot colour, by its kind (`category`). */
export const highlightColor = (category: string | null | undefined): string =>
  HIGHLIGHT_COLORS[category?.toLowerCase() ?? ""] ?? HIGHLIGHT_COLORS.hype;

/** The `i`-th imported chapter's colour. */
export const chapterColor = (i: number): string =>
  CHAPTER_COLORS[((i % CHAPTER_COLORS.length) + CHAPTER_COLORS.length) % CHAPTER_COLORS.length];

/** An activity's colour, by its kind. */
export const activityColor = (kind: string): string => ACTIVITY_COLORS[kind.toLowerCase()] ?? OTHER;

/** Words with their first letter a capital: "just chatting" → "Just chatting". */
const sentence = (s: string) => (s ? s[0].toUpperCase() + s.slice(1) : s);

/** What an activity says on the timeline: "Game: Chess", "Just chatting". */
export function activityLabel(a: Activity): string {
  const name = a.name?.trim();
  return name ? `${sentence(a.kind)}: ${name}` : sentence(a.kind);
}

/** A cast member's role and aliases, as the cast list says them: "Streamer · also Mar, M". */
export function castDetail(m: CastMember): string {
  const aliases = m.aliases.filter((a) => a.trim() && a.toLowerCase() !== m.name.toLowerCase());
  return [sentence(m.role), aliases.length ? `also ${aliases.join(", ")}` : ""].filter(Boolean).join(" · ");
}

/** `hex` (#rrggbb) at `alpha`, as rgb() (not color-mix: the older web views have none). */
export function withAlpha(hex: string, alpha: number): string {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  return `rgb(${r} ${g} ${b} / ${alpha})`;
}
