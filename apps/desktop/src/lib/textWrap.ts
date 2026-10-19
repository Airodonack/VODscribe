// Text onto lines no wider than a width, as measured by the caller (a
// canvas's measureText), for what's drawn over the player (screenOverlay.ts).

/** `text`'s words onto lines no wider than `most` by `width`; a word wider
 *  than that alone is broken where it has to be, rather than run over. */
export function wrapWith(text: string, most: number, width: (s: string) => number): string[] {
  const lines: string[] = [];
  for (const word of text.split(/\s+/).filter(Boolean)) {
    const last = lines.at(-1);
    if (last !== undefined && width(`${last} ${word}`) <= most) {
      lines[lines.length - 1] = `${last} ${word}`;
      continue;
    }
    let rest = word;
    while (width(rest) > most && rest.length > 1) {
      // The longest start of it that fits (at least a character).
      let n = 1;
      while (n < rest.length && width(rest.slice(0, n + 1)) <= most) n++;
      lines.push(rest.slice(0, n));
      rest = rest.slice(n);
    }
    lines.push(rest);
  }
  return lines;
}
