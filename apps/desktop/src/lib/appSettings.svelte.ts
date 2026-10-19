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

  /** Puts every setting back as it starts, after any save under way. */
  async reset() {
    await this.#saving;
    try {
      this.value = await api.resetSettings();
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Changes some settings and saves them all. */
  async update(change: Partial<Settings>) {
    await this.load();
    if (!this.value) return;
    this.value = { ...this.value, ...change };
    const snapshot = $state.snapshot(this.value);
    this.#saving = this.#saving.then(() =>
      api.setSettings(snapshot).then(
        () => void (this.error = null),
        (e) => void (this.error = String(e)),
      ),
    );
    await this.#saving;
  }
}

export const settings = new AppSettings();
