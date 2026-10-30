// "One request at a time, the newest wins", for pictures asked for as the
// pointer moves (the timeline's thumbnail, its exact frame, a piece's new
// edge in an edit, an edit's row's picture): while one is being asked, the
// moments wanted meanwhile wait, and only the newest of them is asked next.
// What comes back is shown, unless something newer is wanted by then; and
// whatever isn't shown, or is shown no more, is let go of (`free`), in one
// place: an image closed, a URL revoked.

export interface Latest<A, T> {
  /** Asks for `arg`'s, once nothing's being asked. */
  fetch: (arg: A) => Promise<T>;
  /** It came, for `arg`, and is shown until the next one or `clear`. */
  got: (value: T, arg: A) => void;
  /** Lets go of what came and isn't shown, or is shown no more. */
  free?: (value: T) => void;
  /** Whether what comes for a moment since moved on from is still shown while
   *  the newest is asked (a thumbnail as the pointer moves: one near beats none). */
  between?: boolean;
  /** Asking for the one still wanted failed. */
  failed?: (error: unknown, arg: A) => void;
}

/** See the top of this file: `want` the newest, `clear` to want nothing (what's shown is let go of). */
export function latest<A, T>(o: Latest<A, T>) {
  let wanted: { arg: A } | null = null;
  let asking = false;
  let shown: { value: T } | null = null;

  const letGo = () => {
    if (shown) o.free?.(shown.value);
    shown = null;
  };
  async function ask() {
    if (asking || !wanted) return;
    asking = true;
    const want = wanted;
    const current = () => wanted === want || (!!o.between && wanted !== null);
    try {
      const value = await o.fetch(want.arg);
      if (!current()) o.free?.(value);
      else {
        const before = shown;
        shown = { value };
        o.got(value, want.arg);
        if (before) o.free?.(before.value);
      }
    } catch (e) {
      if (current()) o.failed?.(e, want.arg);
    } finally {
      asking = false;
      // Moved on while asking: the newest.
      if (wanted && wanted !== want) void ask();
    }
  }
  return {
    want(arg: A) {
      wanted = { arg };
      void ask();
    },
    clear() {
      wanted = null;
      letGo();
    },
  };
}
