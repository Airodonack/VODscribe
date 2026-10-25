// The VOD the per-VOD page (routes/v/[platform]/[id]) has open, for the
// stand-ins to answer from in place of static/demo: the library is that one
// VOD, its transcript the file's segments, its chat the file beside it.
//
// Its chapters, highlights, cast and activity are kept here as the file has
// them, and reach the app's components through the commands the app answers
// for a VOD whose annotations it imported: list_marks (chapters and
// highlights, as marks), vod_annotations (cast, activity and other points of view) and transcript
// (each line's speaker), in core.ts and backend.ts.

import type { ChatLine, Vod } from "desktop/lib/api.ts";
import type { Activity, Annotations, CastMember, Chapter, Highlight, Pov } from "desktop/lib/annotations.ts";
import { chatOf, segmentsOf, vodOf, type Segment } from "./annotated";
import { emit } from "./event";

export interface Annotated {
  vod: Vod;
  segments: Segment[];
  chat: ChatLine[];
  chapters: Chapter[];
  highlights: Highlight[];
  cast: CastMember[];
  activity: Activity[];
  povs: Pov[];
}

let current: Annotated | null = null;
// Each file opened is a VOD of its own to the app's components (and the
// backend's caches, which go by id), so a second one never shows the first's;
// numbered past the demo's, whose caches the same visit may hold.
let lastId = 1000;

/** The VOD open on the page, or null on the landing page's demo. */
export const annotated = (): Annotated | null => current;

/**
 * Opens `annotations` as the library's one VOD, with its chat if it's in
 * (`chat`), or still coming (undefined: `chatArrived` brings it), or none
 * (null). Returns the VOD's id.
 */
export function openAnnotations(annotations: Annotations, chat: unknown | null | undefined): number {
  const lines = chat ? chatOf(chat) : [];
  const id = ++lastId;
  current = {
    vod: vodOf(annotations, id, chat === undefined ? "pending" : chat === null ? "none" : "done"),
    segments: segmentsOf(annotations),
    chat: lines,
    chapters: annotations.chapters,
    highlights: annotations.highlights,
    cast: annotations.cast,
    activity: annotations.activity,
    povs: annotations.povs ?? [],
  };
  return id;
}

/**
 * The chat of the VOD open, arrived after it opened: a list, none (null),
 * or why it couldn't be read. The app hears of it as it hears of a chat
 * fetched (the "vod" event), and reads it again.
 */
export function chatArrived(id: number, chat: unknown | null, failed?: string) {
  if (current?.vod.id !== id) return;
  let lines: ChatLine[] = [];
  let error = failed ?? null;
  if (chat && !error)
    try {
      lines = chatOf(chat);
    } catch (e) {
      error = (e as Error).message;
    }
  current.chat = lines;
  current.vod = {
    ...current.vod,
    chat: error ? "failed" : chat ? "done" : "none",
    chat_error: error,
  };
  emit("vod", current.vod);
}

/** Back to the demo's VODs: the page has closed. */
export function closeAnnotations() {
  current = null;
}
