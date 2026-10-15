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
  if (/^https?:\/\//i.test(item)) return item.replace(/^https?:\/\/(www\.)?/i, "");
  return item.split(/[\\/]/).pop() || item;
}

export class Adder {
  /** What's waiting to be added or being added, newest first, as the library shows VODs. */
  pending = $state<Pending[]>([]);
  #nextKey = 0;
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

  /** Shows stand-ins for `items` until `done` settles. */
  #standIns(items: string[]): () => void {
    const added = items.map((item) => ({ key: this.#nextKey++, label: labelOf(item) }));
    this.pending = [...added.reverse(), ...this.pending];
    const keys = new Set(added.map((p) => p.key));
    return () => (this.pending = this.pending.filter((p) => !keys.has(p.key)));
  }

  /** Adds a pasted link; true if it was added. */
  async link(url: string): Promise<boolean> {
    let ok = false;
    const gone = this.#standIns([url]);
    await this.#enqueue(async (errors) => {
      await api.addVod(url).then(
        () => (ok = true),
        (e) => errors.push(String(e)),
      );
      gone();
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
    const linkGone = links.map((link) => this.#standIns([link]));
    const filesGone = this.#standIns(files);
    void this.#enqueue(async (failed) => {
      for (const [i, link] of links.entries()) {
        await api.addVod(link).catch((e) => failed.push(String(e)));
        linkGone[i]();
      }
      if (files.length) {
        await api.addFiles(files).then(
          (r) => failed.push(...r.errors),
          (e) => failed.push(String(e)),
        );
      }
      filesGone();
    }, errors);
  }
}
