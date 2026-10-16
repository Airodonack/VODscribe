// Back and forward between the app's views (App.svelte): a VOD, an edit, or
// neither. A step is taken when what's open changes, not when the playhead
// moves or a search is typed; each remembers the search box and how its view
// was left (the playhead, the timeline's zoom, how far it was scrolled), which
// the views write here as they go (`note`) and read when brought back
// (`bringBack`). What they noted last also has a VOD or an edit opened any
// other way start where its playhead was left this session (`lastSeen`).

import type { Doc } from "./api";

/** How a view was left, to bring back. */
export interface Left {
  time?: number;
  zoom?: { start: number; span: number };
  scroll?: number;
}

export interface Step {
  doc: Doc | null;
  query: string;
  left: Left;
}

/** The most steps kept each way. */
const KEEP = 100;

const key = (doc: Doc | null) => (doc ? `${doc.kind}:${doc.id}` : "none");
const same = (a: Doc | null, b: Doc | null) => key(a) === key(b);

class Nav {
  back = $state.raw<Step[]>([]);
  forward = $state.raw<Step[]>([]);
  /** The view open now. */
  #here: Doc | null = null;
  /** Set while a step is being taken, so arriving there isn't one of its own. */
  #moving = false;
  /** What each view says of itself now. */
  #now = new Map<string, Left>();
  /** The view being brought back, and how it was left. */
  #bringing: { key: string; left: Left } | null = null;
  #go: (step: Step) => void = () => {};
  #query: () => string = () => "";

  /** How a step is taken (`go`), and where the search box is read from. */
  attach(go: (step: Step) => void, query: () => string) {
    this.#go = go;
    this.#query = query;
  }

  /** View `doc` saying how it is now (its playhead, zoom or scroll). */
  note(doc: Doc | null, part: Left) {
    const k = key(doc);
    this.#now.set(k, { ...this.#now.get(k), ...part });
  }

  /** How view `doc` was when last seen this session, if it was. */
  lastSeen(doc: Doc | null): Left | null {
    return this.#now.get(key(doc)) ?? null;
  }

  /** How view `doc` was left, while it's being brought back by a step; else null. */
  bringBack(doc: Doc | null): Left | null {
    return this.#bringing?.key === key(doc) ? this.#bringing.left : null;
  }

  /** What's open changed (App.svelte): a step, unless one is being taken. */
  arrived(doc: Doc | null) {
    if (this.#moving) {
      this.#moving = false;
      this.#here = doc;
      return;
    }
    if (same(doc, this.#here)) return;
    this.#bringing = null;
    this.back = [...this.back, this.#leaving()].slice(-KEEP);
    this.forward = [];
    this.#here = doc;
  }

  goBack() {
    const step = this.back.at(-1);
    if (!step) return;
    this.back = this.back.slice(0, -1);
    this.forward = [...this.forward, this.#leaving()].slice(-KEEP);
    this.#take(step);
  }

  goForward() {
    const step = this.forward.at(-1);
    if (!step) return;
    this.forward = this.forward.slice(0, -1);
    this.back = [...this.back, this.#leaving()].slice(-KEEP);
    this.#take(step);
  }

  /** Drops the steps to `doc` (it's deleted), and then any to where they'd be already. */
  forget(doc: Doc) {
    const keep = (list: Step[]) => {
      const out: Step[] = [];
      for (const s of list) if (!same(s.doc, doc) && !same(s.doc, out.at(-1)?.doc ?? null)) out.push(s);
      return out;
    };
    this.back = keep(this.back);
    this.forward = keep(this.forward);
    while (this.back.length && same(this.back.at(-1)!.doc, this.#here)) this.back = this.back.slice(0, -1);
    while (this.forward.length && same(this.forward.at(-1)!.doc, this.#here)) this.forward = this.forward.slice(0, -1);
  }

  #leaving(): Step {
    return { doc: this.#here, query: this.#query(), left: { ...this.#now.get(key(this.#here)) } };
  }

  #take(step: Step) {
    this.#bringing = { key: key(step.doc), left: step.left };
    // Arriving where it already is changes nothing to wait for.
    this.#moving = !same(step.doc, this.#here);
    this.#go(step);
  }
}

export const nav = new Nav();
