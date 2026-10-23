// The app's settings (crates/core `settings`), shared by the views that use
// them: loaded once, and saved whole on each change, so changing one setting
// never drops another.

import { api, type Settings } from "./api";

class AppSettings {
  value = $state<Settings | null>(null);
  error = $state<string | null>(null);
  #loading: Promise<void> | null = null;
  // Saves go one after another, in the order made: each is sent on its own
  // thread, so two quick changes could otherwise land older-last, and the
  // next launch would miss the newer one.
  #saving: Promise<void> = Promise.resolve();

  load(): Promise<void> {
    this.#loading ??= api.getSettings().then(
      (s) => void (this.value = s),
      (e) => {
        this.#loading = null;
        this.error = String(e);
      },
    );
    return this.#loading;
  }

  /** Puts every setting back as it starts, in turn with the saves: one asked
   *  for meanwhile is made after it, over the settings as they start. */
  reset(): Promise<void> {
    this.#saving = this.#saving.then(() =>
      api.resetSettings().then(
        (fresh) => {
          this.value = fresh;
          this.error = null;
        },
        (e) => void (this.error = String(e)),
      ),
    );
    return this.#saving;
  }

  /** Changes some settings and saves them all. A change can be worked out
   *  from the settings as they are (`now`): it's then worked out again over
   *  whatever came before it, so it never undoes another one made meanwhile. */
  async update(change: Partial<Settings> | ((now: Settings) => Partial<Settings>)) {
    const changed = (now: Settings) => ({ ...now, ...(typeof change === "function" ? change(now) : change) });
    await this.load();
    if (!this.value) return;
    this.value = changed(this.value);
    // Applied again when its turn comes, over whatever came before it (a reset).
    this.#saving = this.#saving.then(() => {
      if (!this.value) return;
      this.value = changed(this.value);
      return api.setSettings($state.snapshot(this.value)).then(
        () => void (this.error = null),
        (e) => void (this.error = String(e)),
      );
    });
    await this.#saving;
  }
}

export const settings = new AppSettings();
