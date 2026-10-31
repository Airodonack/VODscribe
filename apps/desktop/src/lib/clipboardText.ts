// What Ctrl+C writes to the system's clipboard (copyPaste.ts), and what
// the toasts say of a copy and a paste. A line for each thing copied, as
// the header's "Copy link to this moment" writes a link: its title, the
// stretch of its VOD it starts with, and a site's VOD's link to there; a
// file here is named, never its path (pasted into a chat, a path tells of
// the user's folders). The app's own paste goes by this text: it pastes
// only while the system's clipboard still holds it. Pure: node --test
// apps/desktop/src/lib/clipboardText.test.mjs.

import { formatSpan, formatTime, watchUrl } from "./platform";
import type { Vod } from "./api";

/** A thing copied, as the app says it (src-tauri clipboard.rs `CopiedSaid`). */
export type CopiedSaid = {
  kind: "clip" | "stretch";
  title: string;
  /** The stretch of a VOD it starts with. */
  first: { vodId: number; vodTitle: string; start: number; end: number } | null;
  /** How many pieces it has. */
  pieces: number;
  /** How long it plays, in seconds. */
  length: number;
};

/** `said`'s line: "Title (1:02:03–1:02:45) https://…" for a site's VOD
 *  (`vod`, if it's in the library), "Title (0:01–0:02) — “VOD title”" for
 *  a file here; " + 2 more pieces" if it has more. */
export function copiedLine(said: CopiedSaid, vod?: Vod): string {
  const first = said.first;
  if (!first) return said.title;
  const where = vod?.source_path === null ? watchUrl(vod, first.start) : `— “${vod?.title ?? first.vodTitle}”`;
  const more = said.pieces > 1 ? ` + ${said.pieces - 1} more ${said.pieces > 2 ? "pieces" : "piece"}` : "";
  return `${said.title} (${formatSpan(first.start, first.end)}) ${where}${more}`;
}

/** The text for all of `copied`, a line each. */
export const copiedText = (copied: CopiedSaid[], vodOf: (id: number) => Vod | undefined) =>
  copied.map((c) => copiedLine(c, c.first ? vodOf(c.first.vodId) : undefined)).join("\n");

/** How many of each kind `list` holds: "2 clips", "1 clip and 2 stretches". */
function counted(list: { kind: "clip" | "stretch" }[]): string {
  const clips = list.filter((c) => c.kind === "clip").length;
  const stretches = list.length - clips;
  const n = (k: number, one: string, many: string) => (k ? [`${k} ${k === 1 ? one : many}`] : []);
  return [...n(clips, "clip", "clips"), ...n(stretches, "stretch", "stretches")].join(" and ");
}

/** What the toast says of a copy: "Copied “Title”", "Copied 0:12 of “VOD”", "Copied 2 clips". */
export function copiedSaid(copied: CopiedSaid[]): string {
  const [one] = copied;
  if (copied.length !== 1) return `Copied ${counted(copied)}`;
  if (one.kind === "clip") return `Copied “${one.title}”`;
  return `Copied ${formatTime(one.length)}${one.first ? ` of “${one.first.vodTitle}”` : ""}`;
}

/** What the toast says of a paste: on a bar, of the clips made; in a
 *  timeline, how long what went in plays; and what was left out. */
export function pastedSaid(p: {
  clips: number;
  span: [number, number] | null;
  dropped: number;
  refused: number;
}): string {
  const what = p.span
    ? `Pasted ${formatTime(p.span[1] - p.span[0])}`
    : `Pasted ${p.clips} ${p.clips === 1 ? "clip" : "clips"}`;
  const less = p.dropped ? `, less ${p.dropped} ${p.dropped === 1 ? "piece" : "pieces"} whose VOD is gone` : "";
  const refused = p.refused ? `; ${p.refused} of another VOD left out (those go in an edit)` : "";
  return what + less + refused;
}
