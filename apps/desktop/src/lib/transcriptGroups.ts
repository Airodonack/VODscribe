// The transcript read as a chat window (Transcript.svelte): a speaker's lines
// one after another make a group, under one head with their name and the
// time of the first; the lines in it don't name them again. A group ends
// where someone else speaks (or no one is named), at a chapter's divider, at
// a row that isn't a line (a part not transcribed, a minute of chat), and
// after a pause of more than `GROUP_GAP` seconds, so a long monologue still
// gets fresh times. Pure, so Node tests it (transcriptGroups.test.mjs).

/** A pause longer than this between two lines starts a new group, even for one speaker. */
export const GROUP_GAP = 20;

/** A line as the grouping sees it: when it's said and who says it. */
export interface Said {
  start: number;
  /** Missing in the website's demo: the line is taken to end where it starts. */
  end?: number;
  speaker?: string | null;
}

/** One of the transcript's rows, in order: its line (null for a row that isn't
 *  one), and whether a chapter's divider sits over it. */
export interface GroupItem {
  line: Said | null;
  divided: boolean;
}

/** The head over the first line of a group: who (as `canonical` names them), and when. */
export interface GroupHead {
  name: string;
  start: number;
}

/**
 * For each of `items`, the head it starts a group with, or null (a line in
 * the middle of a group, a line no one is named for, a row that isn't a
 * line). `canonical` gives the name a line's speaker goes by (a cast
 * member's name for one of their aliases, say; by default the name as the
 * line has it): that's the name on the head, and two lines whose names are
 * the same but for case are one speaker's.
 */
export function groupHeads(
  items: GroupItem[],
  canonical: (name: string) => string = (name) => name,
): (GroupHead | null)[] {
  let last: { key: string; end: number } | null = null;
  return items.map(({ line, divided }) => {
    const said = line?.speaker?.trim();
    if (!line || !said) {
      last = null;
      return null;
    }
    const name = canonical(said).trim();
    const key = name.toLowerCase();
    const end = Math.max(line.start, line.end ?? line.start);
    const goesOn = last !== null && !divided && last.key === key && line.start - last.end <= GROUP_GAP;
    last = { key, end };
    return goesOn ? null : { name, start: line.start };
  });
}

/** What stands for someone with no picture: their name's first letter, a capital. */
export function initial(name: string): string {
  const first = Array.from(name.trim())[0] ?? "?";
  return first.toLocaleUpperCase();
}

/** About how tall a group's head is, in pixels, for rows not yet measured. */
export const GROUP_HEAD_HEIGHT = 30;
