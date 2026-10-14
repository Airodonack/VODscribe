// Adding VODs to the library (Library.svelte): a pasted link, links dropped
// as text, and files dropped or picked. Adds queue up, one after another, and
// the errors of everything added together are shown together once the queue is
// done. Links go through the app's check (yt-dlp's probe); files are probed
// with ffmpeg.

import { api } from "./api";

export class Adder {
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

  /** Adds a pasted link; true if it was added. */
  async link(url: string): Promise<boolean> {
    let ok = false;
    await this.#enqueue(async (errors) => {
      await api.addVod(url).then(
        () => (ok = true),
        (e) => errors.push(String(e)),
      );
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
    void this.#enqueue(async (errors) => {
      for (const link of links) await api.addVod(link).catch((e) => errors.push(String(e)));
      if (files.length) {
        await api.addFiles(files).then(
          (r) => errors.push(...r.errors),
          (e) => errors.push(String(e)),
        );
      }
    }, errors);
  }
}
