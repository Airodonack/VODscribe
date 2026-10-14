// The app's settings (crates/core `settings`), shared by the views that use
// them: loaded once, and saved whole on each change, so changing one setting
// never drops another.

import { api, type Settings } from "./api";

class AppSettings {
  value = $state<Settings | null>(null);
  error = $state<string | null>(null);
  #loading: Promise<void> | null = null;

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

  /** Changes some settings and saves them all. */
  async update(change: Partial<Settings>) {
    await this.load();
    if (!this.value) return;
    this.value = { ...this.value, ...change };
    await api.setSettings($state.snapshot(this.value)).then(
      () => (this.error = null),
      (e) => (this.error = String(e)),
    );
  }
}

export const settings = new AppSettings();
