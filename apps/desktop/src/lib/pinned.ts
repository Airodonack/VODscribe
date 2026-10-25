// The transcript's pinned headings (Transcript.svelte): while a chapter's or
// a speaker's lines are at the top of the view and their heading has scrolled
// off above, a copy of the heading stays at the top, the chapter's over the
// speaker's. The next heading coming up pushes the pinned one out by as much
// as they'd overlap, as a contacts list's letters do, and a new chapter
// pushes out both. The transcript lays out only the rows near the view, so
// this works from where the rows are in the list (`offsets`), not from the
// headings on the page. Pure, so Node tests it (pinned.test.mjs).

/** Rows `row` up to (not including) `end`: one speaker's lines under one head, on row `row`. */
export interface Span {
  row: number;
  end: number;
}

/** A group's span, its head `height` tall. */
export interface Group extends Span {
  height: number;
}

/** A chapter's divider over row `row` (or the dividers, where two chapters start there), `height` tall in all. */
export interface Divider {
  row: number;
  height: number;
}

/** What's pinned with the view's top at some place in the list. */
export interface Pinned {
  /** The divider pinned (its place in `dividers`), or -1. */
  chapter: number;
  /** The speaker's group pinned (its place in `groups`), or -1. */
  group: number;
  /** Where the pinned divider's top is, below the view's top: 0, or less while the next chapter pushes it out. */
  chapterTop: number;
  /** Where the pinned speaker's head's top is: under the divider, or higher while the next head pushes it out. */
  speakerTop: number;
  /** How much of the view's top the pinned headings cover. */
  height: number;
}

/** The last of `0..n-1` for which `below` holds (they hold up to some point, then don't), or -1. */
function lastBelow(n: number, below: (i: number) => boolean): number {
  let lo = -1;
  let hi = n - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (below(mid)) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/**
 * The groups of one speaker's lines, from the heads over the rows that
 * start one (transcriptGroups.ts `groupHeads`) and whether each row is a
 * line in a group (one someone is named for; and, where the transcript
 * names anyone, any line, those no one is named for being Unknown's): a
 * group goes on over the rows after its head that are such lines with no
 * head of their own.
 */
export function groupSpans(heads: readonly unknown[], speaking: readonly boolean[]): Span[] {
  const out: Span[] = [];
  let open: Span | null = null;
  heads.forEach((head, i) => {
    if (head || !speaking[i]) {
      if (open) open.end = i;
      open = null;
    }
    if (head) out.push((open = { row: i, end: heads.length }));
  });
  return out;
}

/**
 * What's pinned with the view's top at `y` in the list: `offsets[i]` is where
 * row `i` starts (and the last, where the list ends), and `dividers` and
 * `groups` are in order. A divider is
 * pinned once its top is above the view's; a speaker's head once its top is
 * above the pinned divider's bottom, until their lines end there; and a
 * group is never pinned under another chapter's divider than its own.
 */
export function pinnedAt(
  offsets: ArrayLike<number>,
  dividers: readonly Divider[],
  groups: readonly Group[],
  y: number,
): Pinned {
  const none = { chapter: -1, group: -1, chapterTop: 0, speakerTop: 0, height: 0 };
  if (offsets.length < 2) return none;
  const chapter = lastBelow(dividers.length, (i) => offsets[dividers[i].row] < y);
  const from = chapter >= 0 ? dividers[chapter].row : -1;
  const next = dividers[chapter + 1];
  const chapterHeight = chapter >= 0 ? dividers[chapter].height : 0;
  // Where the next chapter's divider starts: it pushes everything pinned out.
  const chapterEnd = next ? offsets[next.row] : Infinity;
  const line = y + chapterHeight;
  // A head's top is its row's, below the divider when the row has one.
  const headTop = (g: number) => offsets[groups[g].row] + (groups[g].row === from ? chapterHeight : 0);
  // The chapter's last group whose head is above the line, if its lines go
  // on below it: one that runs to the next divider stays as long as the
  // divider pinned over it does, being pushed out with it.
  let group = lastBelow(groups.length, (g) => offsets[groups[g].row] < line && (!next || groups[g].row < next.row));
  if (group >= 0 && headTop(group) >= line) group--;
  if (group >= 0) {
    const end = offsets[groups[group].end];
    if (groups[group].row < from || (end <= line && end < chapterEnd)) group = -1;
  }
  if (chapter < 0 && group < 0) return none;
  const speaker = group >= 0 ? groups[group].height : 0;
  const chapterTop = Math.min(0, chapterEnd - y - chapterHeight - speaker);
  const speakerTop = group >= 0 ? Math.min(chapterHeight + chapterTop, offsets[groups[group].end] - y - speaker) : 0;
  const height = Math.max(0, chapter >= 0 ? chapterHeight + chapterTop : 0, group >= 0 ? speakerTop + speaker : 0);
  return { chapter, group, chapterTop, speakerTop, height };
}

/**
 * How much the pinned headings cover with row `row` at the view's top: what
 * a row brought into view (a search match, the line playing) must be below.
 * None for a row with a divider of its own (it pushes the others out);
 * the divider's, for a row starting a group (its head is in view, under it).
 */
export function reserveAt(row: number, dividers: readonly Divider[], groups: readonly Group[]): number {
  const chapter = lastBelow(dividers.length, (i) => dividers[i].row <= row);
  if (chapter >= 0 && dividers[chapter].row === row) return 0;
  const group = lastBelow(groups.length, (g) => groups[g].row <= row);
  const within = group >= 0 && groups[group].row < row && row < groups[group].end;
  return (chapter >= 0 ? dividers[chapter].height : 0) + (within ? groups[group].height : 0);
}
