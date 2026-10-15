// Adding VODs to the library (Library.svelte): a pasted link, links dropped
// as text, and files dropped or picked. Adds queue up, one after another, and
// the errors of everything added together are shown together once the queue is
// done. Links go through the app's check (yt-dlp's probe); files are probed
// with ffmpeg.

import { api } from "./api";

/** Something being added, shown as a stand-in row until its VOD arrives. */
export interface Pending {
  key: number;
  /** A file's name, or a link's address. */
  label: string;
}

/** What to call `item` (a link or a file path) on its stand-in. */
function labelOf(item: string): string {
  item = item.trim();
  if (/^https?:\/\//i.test(item)) return item.replace(/^https?:\/\/(www\.)?/i, "");
  return item.split(/[\\/]/).pop() || item;
}

export class Adder {
  /** What's waiting to be added or being added, newest first, as the library shows VODs. */
  pending = $state<Pending[]>([]);
  // Also the add's ticket in the app, so it starts somewhere new each launch:
  // a reloaded window's keys don't meet tickets left from before.
  #nextKey = Math.floor(Math.random() * 2 ** 40);
  /** Something is being added: the form waits. */
  adding = $state(false);
  /** What went wrong, one line per failure; also set by the library's other actions. */
  error = $state<string | null>(null);

  #queue: Promise<void> = Promise.resolve();
  #queued = 0;
  #errors: string[] = [];

  /** Runs `job` after what's queued, collecting its failures with `early` (found before it ran). */
  #enqueue(job: (errors: string[]) => Promise<void>, early: string[] = []): Promise<void> {
    if (this.#queued++ === 0) this.#errors = [];
    this.#errors.push(...early);
    this.adding = true;
    this.error = null;
    const done = this.#queue.then(() => job(this.#errors));
    this.#queue = done.finally(() => {
      if (--this.#queued > 0) return;
      this.adding = false;
      this.error = this.#errors.length ? this.#errors.join("\n") : null;
    });
    return this.#queue;
  }

  /** Keys of stand-ins canceled: skipped if not started, stopped or ignored if started. */
  #canceled = new Set<number>();

  /** A stand-in for `item`, shown until it's #done or canceled. */
  #standIn(item: string): Pending {
    const p = { key: this.#nextKey++, label: labelOf(item) };
    this.pending = [p, ...this.pending];
    return p;
  }

  #done(p: Pending) {
    this.pending = this.pending.filter((q) => q.key !== p.key);
    this.#canceled.delete(p.key);
  }

  /**
   * Cancels adding the stand-in `key`: one waiting is skipped; a link being
   * looked up is stopped (its lookup can take a while). A file being read
   * finishes (in a second or so) and is added.
   */
  cancel(key: number) {
    this.#canceled.add(key);
    this.pending = this.pending.filter((p) => p.key !== key);
    void api.cancelAdd(key).catch(() => {});
  }

  /** Adds link `p.label` came from; false if it failed or was canceled. */
  async #addLink(url: string, p: Pending, failed: string[]): Promise<boolean> {
    if (this.#canceled.has(p.key)) {
      this.#done(p);
      return false;
    }
    const ok = await api.addVod(url, p.key).then(
      () => true,
      (e) => {
        if (!this.#canceled.has(p.key)) failed.push(String(e));
        return false;
      },
    );
    this.#done(p);
    return ok;
  }

  /** Adds a pasted link; true if it was added. */
  async link(url: string): Promise<boolean> {
    let ok = false;
    const p = this.#standIn(url);
    await this.#enqueue(async (errors) => {
      ok = await this.#addLink(url, p, errors);
    });
    return ok;
  }

  /**
   * Adds what was dropped or picked: the links in it, and (only from a real
   * file drop or the file picker, `allowFiles`) the files.
   */
  items(items: string[], allowFiles: boolean) {
    const links: string[] = [];
    const files: string[] = [];
    const errors: string[] = [];
    for (const item of items.map((i) => i.trim()).filter(Boolean)) {
      const link = item.match(/https?:\/\/\S+/i)?.[0];
      if (link) links.push(link);
      else if (allowFiles) files.push(item);
      else errors.push(`“${item.length > 80 ? item.slice(0, 80) + "…" : item}” isn't a link.`);
    }
    if (!links.length && !files.length && !errors.length) return;
    const linkStandIns = links.map((link) => this.#standIn(link));
    const fileStandIns = files.map((file) => this.#standIn(file));
    void this.#enqueue(async (failed) => {
      for (const [i, link] of links.entries()) await this.#addLink(link, linkStandIns[i], failed);
      // A file at a time, so one canceled before its turn is skipped.
      for (const [i, file] of files.entries()) {
        const p = fileStandIns[i];
        if (!this.#canceled.has(p.key)) {
          await api.addFiles([file]).then(
            (r) => failed.push(...r.errors),
            (e) => failed.push(String(e)),
          );
        }
        this.#done(p);
      }
    }, errors);
  }
}
