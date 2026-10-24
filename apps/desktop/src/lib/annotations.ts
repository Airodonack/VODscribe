// The annotations file: what a VOD's transcript, chapters, highlights, cast
// and activity look like when they come from outside the app, as one JSON
// per VOD. The app imports it from a file or a URL; the website's per-VOD
// page fetches it. The chat comes as a second file beside it, in the shape
// the app's chat already uses (ChatMessage), so a page is usable before the
// chat arrives. Nothing in here says where a file was made: `produced.build`
// is an opaque id for telling runs apart.

/** The file's `format` field; `version` counts breaking changes to the shape. */
export const ANNOTATIONS_FORMAT = "vodscribe-annotations";
export const ANNOTATIONS_VERSION = 1;

export type AnnotationsSource = {
  platform: "twitch" | "youtube" | "kick" | string;
  /** The platform's own id, as the app stores it (Twitch "v2906977264"). */
  id: string;
  channel: string;
  title: string;
  /** Seconds. */
  duration: number;
  /** ISO 8601, when known. */
  recorded_at: string | null;
};

export type AnnotatedWord = { text: string; start: number; end: number };

export type AnnotatedSegment = {
  id: number;
  start: number;
  end: number;
  text: string;
  words: AnnotatedWord[];
  /** A cast member's `name`, when the text makes the speaker plain. */
  speaker?: string;
};

export type Chapter = { start: number; title: string };

export type HighlightKind = "funny" | "hype" | "drama" | "wholesome" | "lore" | string;

export type Highlight = { start: number; end: number; title: string; kind: HighlightKind; why: string };

export type CastRole = "streamer" | "friend" | "chatter" | "in-game character" | "other" | string;

export type CastMember = { name: string; aliases: string[]; role: CastRole };

export type ActivityKind = "game" | "just chatting" | "event" | "break" | string;

export type Activity = { start: number; end: number; kind: ActivityKind; name?: string };

export type Annotations = {
  format: typeof ANNOTATIONS_FORMAT;
  version: number;
  source: AnnotationsSource;
  produced: { at: string; build: string; models?: Record<string, string> };
  transcript: { segments: AnnotatedSegment[]; corrected: number };
  chapters: Chapter[];
  highlights: Highlight[];
  cast: CastMember[];
  activity: Activity[];
};

/** Where a VOD's files live on the download host, relative to its root. */
export const annotationsPath = (platform: string, id: string) => `annotations/${platform}/${id}.json`;
export const annotationsChatPath = (platform: string, id: string) => `annotations/${platform}/${id}.chat.json`;

/**
 * Reads a parsed JSON value as an annotations file, checking the shape the
 * app relies on; the message of the error it throws is for the user.
 */
export function parseAnnotations(value: unknown): Annotations {
  const bad = (what: string): never => {
    throw new Error(`This isn't a VODscribe annotations file: ${what}.`);
  };
  if (typeof value !== "object" || value === null) bad("not an object");
  const v = value as Record<string, unknown>;
  if (v.format !== ANNOTATIONS_FORMAT) bad(`its format is ${JSON.stringify(v.format)}`);
  if (v.version !== ANNOTATIONS_VERSION) bad(`version ${JSON.stringify(v.version)} isn't ${ANNOTATIONS_VERSION}`);
  const source = (v.source ?? {}) as Record<string, unknown>;
  if (typeof source.platform !== "string" || typeof source.id !== "string") bad("no source platform and id");
  if (typeof source.duration !== "number") bad("no duration");
  const transcript = v.transcript as Record<string, unknown> | undefined;
  if (!transcript || !Array.isArray(transcript.segments)) bad("no transcript segments");
  for (const key of ["chapters", "highlights", "cast", "activity"]) if (!Array.isArray(v[key])) bad(`no ${key} list`);
  return value as Annotations;
}
