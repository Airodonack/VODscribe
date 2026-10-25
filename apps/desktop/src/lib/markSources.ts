// Whose chapters and highlights are shown (the Timeline panel's Marks
// toggles): the user's own, those an annotations import brought, or both.
// The timeline, the chapters and highlights pane and the transcript's
// chapter dividers show only those. Cuts are always shown: they make the
// clips, whoever's the marks around them.

import type { Mark } from "./api";

export interface MarkSources {
  /** The user's own chapters and highlights. */
  user: boolean;
  /** Those an annotations import brought. */
  imported: boolean;
}

/** Both kinds shown: how it starts. */
export const BOTH: MarkSources = { user: true, imported: true };

/** Whether `mark` came with an annotations import (the demo's own have no source: the visitor's). */
export const isImported = (mark: Pick<Mark, "source">) => mark.source === "annotations";

/** The marks of `marks` shown, in their order. */
export function shownMarks<M extends Pick<Mark, "kind" | "source">>(marks: M[], shown: MarkSources): M[] {
  if (shown.user && shown.imported) return marks;
  return marks.filter((m) => m.kind === "cut" || (isImported(m) ? shown.imported : shown.user));
}
