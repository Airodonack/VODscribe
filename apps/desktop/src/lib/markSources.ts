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

/** The toggle `mark` is shown by. */
export const sourceOf = (mark: Pick<Mark, "source">): keyof MarkSources => (isImported(mark) ? "imported" : "user");

/**
 * `mark` with `change` (its title or note, or its moment) made by the user:
 * one an import brought is theirs from then on, as the library keeps it
 * (crates/core db.rs `set_mark_text`, `move_mark`), so the next import leaves it.
 */
export function changed<M extends Pick<Mark, "source">>(mark: M, change: Partial<M>): M {
  return { ...mark, ...change, ...(isImported(mark) ? { source: "user" } : {}) };
}

/** The marks of `marks` shown, in their order. */
export function shownMarks<M extends Pick<Mark, "kind" | "source">>(marks: M[], shown: MarkSources): M[] {
  if (shown.user && shown.imported) return marks;
  return marks.filter((m) => m.kind === "cut" || (isImported(m) ? shown.imported : shown.user));
}
