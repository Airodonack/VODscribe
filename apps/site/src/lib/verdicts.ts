// A VOD page's review (review.svelte.ts): for each chapter and highlight, a
// verdict (keep it, it's wrong, or it's misplaced, with where it should
// start) and a note, and a note for the VOD as a whole. Kept in the browser
// per VOD and written out as a JSONL file, one line per row judged and then
// one for the VOD.

export type Verdict = "keep" | "wrong" | "misplaced";

export const VERDICTS: { value: Verdict; label: string; hint: string }[] = [
  { value: "keep", label: "Keep", hint: "It's right" },
  { value: "wrong", label: "Wrong", hint: "It shouldn't be there, or says the wrong thing" },
  { value: "misplaced", label: "Misplaced", hint: "Right, but it should start elsewhere" },
];

/** What's said of one row. */
export interface Judgment {
  verdict: Verdict | null;
  /** Where a misplaced one should start, in seconds. */
  corrected_start: number | null;
  note: string;
}

/** What's said of a VOD: its rows' judgments by `rowKey`, and its own note. */
export interface Reviewed {
  rows: Record<string, Judgment>;
  note: string;
}

/** A row as the review knows it (annotationRows.ts has more). */
export interface ReviewRow {
  kind: "chapter" | "highlight";
  start: number;
  end: number | null;
  title: string;
}

export const NONE: Judgment = { verdict: null, corrected_start: null, note: "" };

/** Where VOD `platform`/`id`'s review is kept in the browser. */
export const storageKey = (platform: string, id: string) => `vodscribe.review.${platform}.${id}`;

/** The file a VOD's verdicts are saved as. */
export const fileName = (platform: string, id: string) => `verdicts-${platform}-${id}.jsonl`.replace(/[^\w.-]+/g, "_");

/** A row's key in what's kept: its kind, start and title, which a page made
 *  again from the same file gives it again (mark ids don't last a visit). */
export const rowKey = (row: ReviewRow) => `${row.kind}@${row.start}@${row.title}`;

/** Whether anything is said of a row. */
export const isJudged = (j: Judgment | undefined): j is Judgment => !!j && (j.verdict !== null || !!j.note.trim());

const VALUES = new Set<string>(VERDICTS.map((v) => v.value));

/** What was kept (`text`, from storage), or an empty review where there's
 *  nothing or it can't be read. */
export function parseReviewed(text: string | null | undefined): Reviewed {
  const empty: Reviewed = { rows: {}, note: "" };
  if (!text) return empty;
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return empty;
  }
  if (!value || typeof value !== "object") return empty;
  const v = value as { rows?: unknown; note?: unknown };
  const rows: Record<string, Judgment> = {};
  if (v.rows && typeof v.rows === "object")
    for (const [key, j] of Object.entries(v.rows as Record<string, unknown>)) {
      if (!j || typeof j !== "object") continue;
      const { verdict, corrected_start, note } = j as Record<string, unknown>;
      rows[key] = {
        verdict: typeof verdict === "string" && VALUES.has(verdict) ? (verdict as Verdict) : null,
        corrected_start: typeof corrected_start === "number" && corrected_start >= 0 ? corrected_start : null,
        note: typeof note === "string" ? note : "",
      };
    }
  return { rows, note: typeof v.note === "string" ? v.note : "" };
}

/** The verdicts file's lines for VOD `platform`/`id`: each of `rows` with
 *  something said of it, in the order given, then the VOD's own. A row's
 *  verdict is null when it has only a note; a corrected start goes with a
 *  misplaced one only, and an end with a highlight. */
export function verdictLines(platform: string, id: string, rows: ReviewRow[], reviewed: Reviewed): string[] {
  const lines: string[] = [];
  for (const row of rows) {
    const j = reviewed.rows[rowKey(row)];
    if (!isJudged(j)) continue;
    lines.push(
      JSON.stringify({
        platform,
        id,
        kind: row.kind,
        start: row.start,
        ...(row.kind === "highlight" && row.end !== null ? { end: row.end } : {}),
        title: row.title,
        verdict: j.verdict,
        ...(j.verdict === "misplaced" && j.corrected_start !== null ? { corrected_start: j.corrected_start } : {}),
        note: j.note.trim(),
      }),
    );
  }
  lines.push(JSON.stringify({ platform, id, kind: "vod", note: reviewed.note.trim() }));
  return lines;
}

/** A time typed as "1:02:05", "2:05", "125" or "125.5", in seconds; null if it isn't one. */
export function parseTime(text: string): number | null {
  const parts = text.trim().split(":");
  if (parts.length > 3 || parts.some((p) => !/^\d+(\.\d+)?$/.test(p))) return null;
  if (parts.slice(0, -1).some((p) => p.includes("."))) return null;
  return parts.reduce((total, p) => total * 60 + Number(p), 0);
}
