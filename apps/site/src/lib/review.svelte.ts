// The review of the VOD a VOD page shows (VodView.svelte): turned on with
// Review in the site's bar, it puts keep / wrong / misplaced and a note under
// each chapter and highlight in the annotations pane (ReviewControls.svelte),
// and a note for the whole VOD above them. What's said is kept in this
// browser for the VOD (verdicts.ts); the page works the same where it can't
// be. Export verdicts saves it as a JSONL file.

import {
  NONE,
  fileName,
  isJudged,
  parseReviewed,
  rowKey,
  storageKey,
  verdictLines,
  type Judgment,
  type Reviewed,
  type ReviewRow,
} from "./verdicts.ts";

export class Review {
  /** Whether the controls show. */
  on = $state(false);
  #reviewed = $state<Reviewed>({ rows: {}, note: "" });
  readonly platform: string;
  readonly id: string;
  readonly #rows: () => ReviewRow[];
  readonly #key: string;

  /** VOD `platform`/`id`'s review, whose rows (in the file's order) are `rows`'s. */
  constructor(platform: string, id: string, rows: () => ReviewRow[]) {
    this.platform = platform;
    this.id = id;
    this.#rows = rows;
    this.#key = storageKey(platform, id);
    try {
      this.#reviewed = parseReviewed(localStorage.getItem(this.#key));
    } catch {
      // Storage refused (a private window, say): the review lasts the visit.
    }
  }

  /** What's said of `row`. */
  of(row: ReviewRow): Judgment {
    return this.#reviewed.rows[rowKey(row)] ?? NONE;
  }

  /** Says `change` of `row`. */
  set(row: ReviewRow, change: Partial<Judgment>) {
    const key = rowKey(row);
    this.#reviewed.rows[key] = { ...this.of(row), ...change };
    this.#save();
  }

  get note(): string {
    return this.#reviewed.note;
  }
  set note(text: string) {
    this.#reviewed.note = text;
    this.#save();
  }

  /** How many rows have something said of them. */
  get count(): number {
    return this.#rows().filter((row) => isJudged(this.#reviewed.rows[rowKey(row)])).length;
  }

  /** Forgets everything said of this VOD. */
  clear() {
    this.#reviewed = { rows: {}, note: "" };
    try {
      localStorage.removeItem(this.#key);
    } catch {
      // Nothing was kept.
    }
  }

  /** Saves the verdicts file, through a download. */
  export() {
    const text = verdictLines(this.platform, this.id, this.#rows(), this.#reviewed).join("\n") + "\n";
    const url = URL.createObjectURL(new Blob([text], { type: "application/x-ndjson" }));
    const link = document.createElement("a");
    link.href = url;
    link.download = fileName(this.platform, this.id);
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  #save() {
    try {
      localStorage.setItem(this.#key, JSON.stringify(this.#reviewed));
    } catch {
      // Storage refused or full: the review lasts the visit.
    }
  }
}
