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
  /** When the VOD started (its time 0), ISO 8601, when known: the app keeps it as the VOD's `started_at` if nothing better says. */
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

export type Chapter = {
  start: number;
  title: string;
  /** What the chapter is about, as one emoji (a game, cooking, a drive…), shown as given beside its title. */
  icon?: string;
};

/**
 * The kinds of highlight there are, each with the one emoji it shows as (in
 * the app's pane and on its timeline, and on the website's VOD page). The
 * kind's word is what a file carries; the emoji is only how it's drawn, so a
 * file may carry a kind not listed here: it shows as `UNKNOWN_GLYPH` and keeps
 * its word. All are old enough to draw on every system.
 */
export const HIGHLIGHT_GLYPHS = {
  funny: "😂",
  hype: "🔥",
  drama: "🍿",
  wholesome: "💛",
  clutch: "🎯",
  fail: "💀",
  scary: "👻",
  rage: "😡",
  sad: "😢",
  cringe: "😬",
  lore: "📜",
  chat: "💬",
  music: "🎵",
  milestone: "🏆",
  /** A moment worth seeing rather than hearing: a play, a face, a scene. */
  visual: "👀",
} as const;

/** What a highlight of a kind not in `HIGHLIGHT_GLYPHS` shows as. */
export const UNKNOWN_GLYPH = "⭐";

/** A highlight's kind: one of `HIGHLIGHT_GLYPHS`', or another a newer file brings. */
export type HighlightKind = keyof typeof HIGHLIGHT_GLYPHS | string;

/** The emoji a highlight of `kind` shows as, its word in any case. */
export function highlightGlyph(kind: string | null | undefined): string {
  const key = kind?.trim().toLowerCase() ?? "";
  return Object.prototype.hasOwnProperty.call(HIGHLIGHT_GLYPHS, key)
    ? HIGHLIGHT_GLYPHS[key as keyof typeof HIGHLIGHT_GLYPHS]
    : UNKNOWN_GLYPH;
}

export type Highlight = {
  start: number;
  end: number;
  title: string;
  kind: HighlightKind;
  why: string;
  /** Who is in it: cast members' `name`s, or their aliases. */
  cast?: string[];
};

export type CastRole = "streamer" | "friend" | "chatter" | "in-game character" | "other" | string;

/** The sites a cast member's account can be on. */
export const ACCOUNT_PLATFORMS = ["twitch", "kick", "youtube"] as const;

/**
 * Where a cast member streams: the site, and their login there (Twitch's and
 * Kick's channel name; YouTube's "@handle" or channel id). Never a picture's
 * address: pictures are asked for when shown (accounts.ts).
 */
export type Account = { platform: (typeof ACCOUNT_PLATFORMS)[number]; login: string };

export type CastMember = {
  name: string;
  aliases: string[];
  role: CastRole;
  /** The streamer behind a character, by their public name, when it isn't `name`. */
  player?: string;
  account?: Account;
};

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
  // A highlight's cast may be left out, but what's there is a list of names (as the app's import reads it).
  for (const h of v.highlights as { cast?: unknown }[])
    if (h?.cast !== undefined && !(Array.isArray(h.cast) && h.cast.every((n) => typeof n === "string")))
      bad("a highlight's cast isn't a list of names");
  // What the file may leave out is filled in as crates/core annotations.rs fills it in (its
  // `#[serde(default)]`s are the rule): a segment's words, a highlight's why and cast, a cast member's aliases.
  const segments = (transcript!.segments as Record<string, unknown>[]).map((g) =>
    typeof g !== "object" || g === null ? g : { ...g, words: g.words ?? [] },
  );
  const highlights = (v.highlights as Record<string, unknown>[]).map((h) =>
    typeof h !== "object" || h === null ? h : { ...h, why: h.why ?? "", cast: h.cast ?? [] },
  );
  // A chapter's icon that isn't one emoji is left out, rather than the file refused.
  const chapters = (v.chapters as Record<string, unknown>[]).map((c) => {
    if (typeof c !== "object" || c === null || !("icon" in c)) return c;
    const { icon, ...rest } = c;
    const one = oneEmoji(icon);
    return one ? { ...rest, icon: one } : rest;
  });
  // A cast member's player that isn't a name, or account on a site not listed
  // or with a login that can't be one, is left out rather than the file refused.
  const cast = (v.cast as Record<string, unknown>[]).map((m) => {
    if (typeof m !== "object" || m === null) return m;
    const { player, account, ...rest } = m;
    const name = typeof player === "string" ? player.trim() : "";
    const tied = readAccount(account);
    return {
      ...rest,
      aliases: rest.aliases ?? [],
      ...(name ? { player: name } : {}),
      ...(tied ? { account: tied } : {}),
    };
  });
  return { ...v, transcript: { ...transcript, segments }, chapters, highlights, cast } as Annotations;
}

/**
 * `value` as a cast member's account: a listed platform and a login of
 * letters, digits and `_.-` (a YouTube handle with its "@" first), trimmed;
 * null for anything else. crates/core annotations.rs `Account::read` is the
 * same rule.
 */
export function readAccount(value: unknown): Account | null {
  if (typeof value !== "object" || value === null) return null;
  const { platform, login } = value as Record<string, unknown>;
  if (typeof platform !== "string" || typeof login !== "string") return null;
  const site = platform.trim().toLowerCase();
  const name = login.trim();
  if (!(ACCOUNT_PLATFORMS as readonly string[]).includes(site)) return null;
  if (!(site === "youtube" ? /^@?[A-Za-z0-9_.-]+$/ : /^[A-Za-z0-9_.-]+$/).test(name)) return null;
  return { platform: site as Account["platform"], login: name };
}

/**
 * `value` as a chapter's icon: one emoji (a single character as the reader
 * sees it, of no letter or digit and not plain ASCII), trimmed; null for
 * anything else. crates/core annotations.rs `one_emoji` is the same rule.
 */
export function oneEmoji(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const s = value.trim();
  if (!s || /^[ -~]*$/.test(s) || /[\p{L}\p{N}\p{Cc}\p{Z}]/u.test(s)) return null;
  const graphemes = [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(s)];
  return graphemes.length === 1 ? s : null;
}
