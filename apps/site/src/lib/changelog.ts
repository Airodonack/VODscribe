// The releases page (routes/releases) reads CHANGELOG.md at build time: each
// "## <version> — <date>" heading, the "- " changes under it, and any plain
// text before them ("First release."). A version still being worked on
// ("## 0.3.0 (unreleased)") is left out, since the site is published on every
// push, before the release is.

export interface Release {
  version: string;
  /** YYYY-MM-DD. */
  date: string;
  intro: string[];
  changes: string[];
}

export function releases(markdown: string): Release[] {
  const out: Release[] = [];
  let current: Release | null = null;
  for (const line of markdown.split("\n")) {
    const heading = line.match(/^## (\d+\.\d+\.\d+)(.*)$/);
    if (heading) {
      const date = heading[2].match(/(\d{4}-\d{2}-\d{2})/)?.[1];
      current = date && !/unreleased/i.test(heading[2]) ? { version: heading[1], date, intro: [], changes: [] } : null;
      if (current) out.push(current);
    } else if (line.startsWith("## ")) {
      current = null;
    } else if (current && line.startsWith("- ")) {
      current.changes.push(line.slice(2).trim());
    } else if (current && /^\s+\S/.test(line) && current.changes.length) {
      // A change wrapped onto the next line.
      current.changes[current.changes.length - 1] += " " + line.trim();
    } else if (current && line.trim() && !current.changes.length) {
      current.intro.push(line.trim());
    }
  }
  return out;
}

/** A change's text as plain runs and `code` runs. */
export function runs(text: string): { code: boolean; text: string }[] {
  return text
    .split(/(`[^`]+`)/)
    .filter(Boolean)
    .map((part) => (part.startsWith("`") ? { code: true, text: part.slice(1, -1) } : { code: false, text: part }));
}

/** "2026-10-13" as "October 13, 2026". */
export function longDate(date: string): string {
  const [y, m, d] = date.split("-").map(Number);
  const month = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
  ][m - 1];
  return `${month} ${d}, ${y}`;
}
