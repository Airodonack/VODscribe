// App updates: checked at launch and once a day, installed only when the user
// says so. Each update is signed; the updater refuses one that doesn't match
// the public key in tauri.conf.json (plugins.updater).

import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

const DAY = 24 * 60 * 60 * 1000;

class Updates {
  /** The newer version, once a check finds one. */
  available = $state<Update | null>(null);
  checking = $state(false);
  installing = $state(false);
  /** Fraction downloaded while installing, or null while the size is unknown. */
  progress = $state<number | null>(null);
  error = $state<string | null>(null);
  /** What the last check from Settings found, when there's nothing to install. */
  status = $state<string | null>(null);
  /** Whether a VOD is downloading or transcribing, which the restart interrupts (set by App). */
  busy = false;

  /** Checks now and daily. Development builds don't check. */
  start(): () => void {
    if (import.meta.env.DEV) return () => {};
    this.check(false);
    const timer = setInterval(() => this.check(false), DAY);
    return () => clearInterval(timer);
  }

  /** `manual` checks (from Settings) report their result; automatic ones fail quietly. */
  async check(manual: boolean) {
    if (this.checking || this.installing) return;
    this.checking = true;
    if (manual) {
      this.status = null;
      this.error = null;
    }
    try {
      this.available = await check();
      if (manual && !this.available) this.status = "VODscribe is up to date.";
    } catch (e) {
      if (manual) this.error = `Couldn't check for updates: ${e}`;
    } finally {
      this.checking = false;
    }
  }

  /** Downloads, checks and installs the update, then restarts into it. */
  async install() {
    const update = this.available;
    if (!update || this.installing) return;
    if (
      this.busy &&
      !confirm("VODscribe restarts to update. The VOD in progress carries on from where it was after the restart.")
    )
      return;
    this.installing = true;
    this.error = null;
    this.progress = null;
    let total = 0;
    let done = 0;
    try {
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        else if (e.event === "Progress") {
          done += e.data.chunkLength;
          this.progress = total ? done / total : null;
        }
      });
      // Windows has already closed the app to run the installer by now.
      await relaunch();
    } catch (e) {
      this.error = `The update couldn't be installed (${e}). You can download it from vodscribe.com instead.`;
      this.installing = false;
    }
  }
}

export const updates = new Updates();
