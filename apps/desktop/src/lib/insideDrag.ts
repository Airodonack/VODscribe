// A drag that started inside the page (transcript text, a thumbnail, a
// streamer card's link) isn't a drop to add, and this tells which native
// drops are one. Tested by itself: insideDrag.test.mjs.
//
// The page hears such a drag twice: its own DOM events (dragstart, dragend),
// straight from the web view, and the window's native events (enter, leave,
// drop: Tauri's, or the app's own drop target's on Windows and macOS), which
// come later, through an emit. On Windows a drag shorter than that delay has
// its native enter and drop arrive after dragend. So what the drag carried is
// kept until its own native events are over: the first native enter after
// dragstart is the drag's, and it's over once that is closed by a drop or a
// leave and dragend has come (a drag carried out of the window is closed by a
// leave before dragend). A native enter after that is another drag's, from
// outside, and the same link dropped by it is added.

export class InsideDrag {
  /** Whether the page's own drag is going on (dragstart, not yet dragend). */
  #dragging = false;
  /** What it carried, a line each. */
  #carried: string[] = [];
  /** Its native events: none yet ("waiting"), entered ("over"), left or dropped ("closed"); none of its own (null). */
  #native: "waiting" | "over" | "closed" | null = null;

  /** The page's dragstart, with the drag's text (uri-list or plain). */
  start(text: string) {
    this.#dragging = true;
    this.#carried = text
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter(Boolean);
    this.#native = "waiting";
  }

  /** The page's dragend. */
  end() {
    this.#dragging = false;
    if (this.#native === "closed") this.#forget();
  }

  /**
   * A native enter carrying `paths`: whether it's the page's own drag. One
   * that comes after dragend is the drag's only if it carries what the drag
   * did: a drag the window never hears (transcript text, on Windows and
   * Linux) leaves it waiting, and the next drag from outside isn't the page's.
   */
  enter(paths: string[]): boolean {
    const carried = paths.length > 0 && paths.every((p) => this.#carried.includes(p.trim()));
    const waiting = this.#native === "waiting" && (this.#dragging || carried);
    if (waiting || (this.#dragging && this.#native === "closed")) {
      this.#native = "over";
      return true;
    }
    if (!this.#dragging && this.#native !== "over") this.#forget();
    return this.#dragging || this.#native === "over";
  }

  /** A native leave: whether it was the page's own drag. */
  leave(): boolean {
    return this.#close();
  }

  /** A native drop of `paths`: those to add (none from the page's own drag, while it goes on). */
  drop(paths: string[]): string[] {
    const carried = this.#carried;
    const own = this.#close();
    if (own && this.#dragging) return [];
    return own ? paths.filter((p) => !carried.includes(p.trim())) : paths;
  }

  #close(): boolean {
    if (this.#native !== "over") return this.#dragging;
    this.#native = "closed";
    if (!this.#dragging) this.#forget();
    return true;
  }

  #forget() {
    this.#carried = [];
    this.#native = null;
  }
}
