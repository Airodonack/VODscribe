// Back and forward between the app's views (App.svelte): a VOD, an edit, or
// neither, each either as itself (its transcript) or with a search's results
// in its place. A step is taken when what's open changes, or the results come
// up or go, not when the playhead moves or a search is retyped; each
// remembers the search (its words and filter) and how its view was left (the
// playhead, the timeline's zoom, how far the transcript or the results were
// scrolled), which the views write here as they go (`note`) and read when
// brought back (`bringBack`). What they noted last also has a VOD or an edit
// opened any other way start where its playhead was left this session
// (`lastSeen`).

import { docKey, type Doc } from "./api";

/** How a view was left, to bring back. */
export interface Left {
  time?: number;
  zoom?: { start: number; span: number };
  /** How far its transcript was scrolled. */
  scroll?: number;
  /** How far the search's results over it were scrolled. */
  results?: number;
}

/** The search box and its filter. */
export interface Search {
  query: string;
  /** Every VOD, rather than just the one open. */
  all: boolean;
  audio: boolean;
  chat: boolean;
  /** The filter's window, in seconds, and who (empty for anyone). */
  within: number;
  from: string;
  /** Its results set aside for the view under them (a match double-clicked),
   *  the words kept in the box. */
  aside: boolean;
}

export interface Step {
  doc: Doc | null;
  search: Search;
  left: Left;
}

/** The most steps kept each way. */
const KEEP = 100;

const key = (doc: Doc | null) => (doc ? docKey(doc) : "none");
const same = (a: Doc | null, b: Doc | null) => key(a) === key(b);
/** Whether a search's results are up, rather than the view itself. */
export const searching = (search: Search) => search.query.trim() !== "" && !search.aside;
/** The same results (the same words, whatever the spaces round them), or both the view itself. */
const sameSearch = (a: Search, b: Search) =>
  searching(a) === searching(b) && (!searching(a) || a.query.trim() === b.query.trim());
/** Two steps that would bring back the same thing. */
const samePlace = (a: Step, b: Step) => same(a.doc, b.doc) && sameSearch(a.search, b.search);

class Nav {
  back = $state.raw<Step[]>([]);
  forward = $state.raw<Step[]>([]);
  /** The view open now, and the search over it. */
  #here: Doc | null = null;
  #search: Search = { query: "", all: false, audio: true, chat: true, within: 20, from: "", aside: false };
  /** Set while a step is being taken, so arriving there isn't one of its own. */
  #moving = false;
  /** What each view says of itself now. */
  #now = new Map<string, Left>();
  /** The view being brought back, and how it was left. */
  #bringing: { key: string; left: Left } | null = null;
  #go: (step: Step) => void = () => {};

  /** How a step is taken. */
  attach(go: (step: Step) => void) {
    this.#go = go;
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

  /** What's open, or the search over it, changed (App.svelte): a step when
   *  the view changed or the results came up or went, unless one is being
   *  taken. A search retyped or refiltered is the same results, newer. */
  arrived(doc: Doc | null, search: Search) {
    if (this.#moving) {
      this.#moving = false;
      this.#here = doc;
      this.#search = search;
      return;
    }
    if (same(doc, this.#here) && searching(search) === searching(this.#search)) {
      this.#search = search;
      return;
    }
    this.#bringing = null;
    this.back = [...this.back, this.#leaving()].slice(-KEEP);
    this.forward = [];
    this.#here = doc;
    this.#search = search;
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

  /** Drops the steps to `doc` (it's deleted), and then any to where they'd be already;
   *  open now, it's nowhere, so leaving it is no step back to it. A search's
   *  results over it that looked through every VOD stay, over nothing open
   *  (their rows from it are gone, as the search runs again); those of it alone go. */
  forget(doc: Doc) {
    if (same(doc, this.#here)) this.#here = null;
    const here = this.#leaving();
    const keep = (list: Step[]) => {
      const out: Step[] = [];
      for (const s of list) {
        const step = !same(s.doc, doc) ? s : searching(s.search) && s.search.all ? { ...s, doc: null } : null;
        if (step && !(out.length && samePlace(step, out.at(-1)!))) out.push(step);
      }
      return out;
    };
    this.back = keep(this.back);
    this.forward = keep(this.forward);
    while (this.back.length && samePlace(this.back.at(-1)!, here)) this.back = this.back.slice(0, -1);
    while (this.forward.length && samePlace(this.forward.at(-1)!, here)) this.forward = this.forward.slice(0, -1);
  }

  #leaving(): Step {
    return { doc: this.#here, search: this.#search, left: { ...this.#now.get(key(this.#here)) } };
  }

  #take(step: Step) {
    this.#bringing = { key: key(step.doc), left: step.left };
    // Arriving where it already is changes nothing to wait for.
    this.#moving = !samePlace(step, this.#leaving());
    this.#go(step);
  }
}

export const nav = new Nav();
