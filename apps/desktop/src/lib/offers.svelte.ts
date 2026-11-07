// Annotations published for a VOD, offered (AnnotationsOffer.svelte): when
// the app finds some for a VOD just added (crates/core
// `annotations::look_up`), from the gift button in its library row, or from
// "Check for a VODscription" in the row's menu. At every start, and from "Check all
// VODs for VODscriptions" in that menu, the app checks the whole library against
// the file host's list of published annotations (crates/core
// `annotations_filter`): a VOD found shows its gift button, marked when its
// annotations are newer than the ones it brought in. Using them fetches the file and
// imports it, as an annotations file picked would be: the VOD's transcript
// and the marks an import brought are replaced, the user's own marks stay.

import { api, type AnnotationsOffer, type Vod } from "./api";
import { settings } from "./appSettings.svelte";
import { shows } from "./asks";
import { toast } from "./toasts.svelte";

export interface Offered {
  vod: Vod;
  offer: AnnotationsOffer;
  /** Offered because it was just added: it has a "Don't ask again" box. */
  afterAdd: boolean;
  /** Why the file host wasn't asked just now (asking is off, or it failed), when the
   *  offer goes by the last check: the dialog says so. Null when it was asked. */
  unasked: string | null;
}

class Offers {
  /** The offer shown, if any. */
  shown = $state<Offered | null>(null);
  /** Being fetched and imported. */
  busy = $state(false);
  /** The file host being asked, for the gift button. */
  asking = $state(false);
  error = $state<string | null>(null);

  /** Offers `vod`'s annotations, as last found, unless something else is
   *  offered already, or it was just added and the offer is turned off
   *  (`ask_to_use_vodscription`: its gift button still offers them). */
  async offer(vod: Vod, afterAdd: boolean, unasked: string | null = null) {
    if (this.shown || (afterAdd && !shows(settings.value, "ask_to_use_vodscription"))) return;
    const offer = await api.annotationsOffer(vod.id).catch(() => null);
    if (!offer || offer.available !== "available" || this.shown) return;
    this.#show({ vod, offer, afterAdd, unasked });
  }

  #show(offered: Offered) {
    this.error = null;
    this.shown = offered;
  }

  /** The gift button: asks the file host first (when the user lets the app ask),
   *  so "the latest already" is what the host says now, then offers them. */
  async gift(vod: Vod) {
    if (this.shown || this.asking) return;
    this.asking = true;
    try {
      const { asked, offer } = await api.checkAnnotations(vod.id);
      if (this.shown) return;
      if (!asked)
        return await this.offer(
          vod,
          false,
          "VODscribe didn't ask the file host again: that's turned off in Settings › Privacy.",
        );
      if (offer.available !== "available") toast.show("There's no VODscription for this VOD any more.");
      else this.#show({ vod, offer, afterAdd: false, unasked: null });
    } catch {
      await this.offer(vod, false, "VODscribe couldn't reach the file host just now.");
    } finally {
      this.asking = false;
    }
  }

  /** Checks the whole library (the row menu's "Check all VODs for VODscriptions"), and says what's found. */
  async checkLibrary() {
    try {
      const { asked, found, newer } = await api.checkLibraryAnnotations();
      if (!asked) return toast.show("VODscription is turned off in Settings › Privacy.");
      const parts = [found.length && offered("", found.length), newer.length && offered("newer ", newer.length)].filter(
        Boolean,
      );
      toast.show(
        parts.length
          ? `Found ${parts.join(" and ")}: use the gift button in the library.`
          : "No new VODscriptions for the VODs in your library.",
      );
    } catch {
      toast.show("VODscribe couldn't reach the file host just now.");
    }
  }

  /** Asks the file host again about `vod` (its row's menu), and offers what's found. */
  async check(vod: Vod) {
    try {
      const { asked, can_have, offer } = await api.checkAnnotations(vod.id);
      if (!can_have) toast.show("VODscription is only for Twitch, YouTube and Kick VODs.");
      else if (!asked) toast.show("VODscription is turned off in Settings › Privacy.");
      else if (offer.available !== "available") toast.show("There's no VODscription for this VOD yet.");
      else await this.offer(vod, false);
    } catch (e) {
      toast.show(String(e));
    }
  }

  /** Use, and with `hide` ("Don't ask again" ticked) stop offering after an add. */
  async use(hide = false) {
    const shown = this.shown;
    if (!shown || this.busy) return;
    if (hide && shown.afterAdd) void this.#stop();
    this.busy = true;
    this.error = null;
    try {
      // The page reads it again on the app's word (`annotations-imported`).
      await api.fetchAnnotations(shown.vod.id);
      if (this.shown === shown) this.shown = null;
      toast.show(`“${shown.vod.title}” has its VODscription now.`);
    } catch (e) {
      // Closed meanwhile: said in a toast instead.
      if (this.shown === shown) this.error = String(e);
      else toast.show(String(e));
    } finally {
      this.busy = false;
    }
  }

  /** "Don't ask again", answered: VODs added from now on aren't offered theirs.
   *  The app still looks them up (`ask_for_annotations`), for the gift button. */
  async #stop() {
    await settings.update({ ask_to_use_vodscription: false });
  }

  /** Closes it (Not now), and with `hide` ("Don't ask again" ticked) stops
   *  offering after an add; one being fetched carries on, and says when it's done. */
  close(hide = false) {
    if (hide && this.shown?.afterAdd) void this.#stop();
    this.shown = null;
    this.error = null;
  }
}

/** "a VODscription for 1 VOD", "newer VODscriptions for 3 VODs": what the library check found. */
const offered = (kind: string, n: number) =>
  n === 1 ? `a ${kind}VODscription for 1 VOD` : `${kind}VODscriptions for ${n} VODs`;

export const offers = new Offers();
