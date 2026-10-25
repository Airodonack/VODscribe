// Annotations published for a VOD, offered (AnnotationsOffer.svelte): when
// the app finds some for a VOD just added (crates/core
// `annotations::look_up`), from the gift button in its library row, or from
// "Check for annotations" in the row's menu. Using them fetches the file and
// imports it, as an annotations file picked would be: the VOD's transcript
// and the marks an import brought are replaced, the user's own marks stay.

import { api, type AnnotationsOffer, type Vod } from "./api";
import { settings } from "./appSettings.svelte";
import { toast } from "./toasts.svelte";

export interface Offered {
  vod: Vod;
  offer: AnnotationsOffer;
  /** Offered because it was just added: "Don't ask again" is one of the answers. */
  afterAdd: boolean;
}

class Offers {
  /** The offer shown, if any. */
  shown = $state<Offered | null>(null);
  /** Being fetched and imported. */
  busy = $state(false);
  error = $state<string | null>(null);
  /** "Don't ask again" was picked: the dialog says where that's undone, then closes. */
  stopped = $state(false);

  /** Offers `vod`'s annotations, unless something else is offered already. */
  async offer(vod: Vod, afterAdd: boolean) {
    if (this.shown) return;
    const offer = await api.annotationsOffer(vod.id).catch(() => null);
    if (!offer || offer.available !== "available" || this.shown) return;
    this.error = null;
    this.stopped = false;
    this.shown = { vod, offer, afterAdd };
  }

  /** Asks the file host again about `vod` (its row's menu), and offers what's found. */
  async check(vod: Vod) {
    try {
      const { asked, offer } = await api.checkAnnotations(vod.id);
      if (!asked) toast.show("Looking for annotations is turned off in Settings › Privacy.");
      else if (offer.available !== "available") toast.show("No annotations are published for this VOD yet.");
      else await this.offer(vod, false);
    } catch (e) {
      toast.show(String(e));
    }
  }

  async use() {
    const shown = this.shown;
    if (!shown || this.busy) return;
    this.busy = true;
    this.error = null;
    try {
      // The page reads it again on the app's word (`annotations-imported`).
      await api.fetchAnnotations(shown.vod.id);
      if (this.shown === shown) this.shown = null;
      toast.show(`The transcript and annotations are in “${shown.vod.title}”.`);
    } catch (e) {
      // Closed meanwhile: said in a toast instead.
      if (this.shown === shown) this.error = String(e);
      else toast.show(String(e));
    } finally {
      this.busy = false;
    }
  }

  /** "Don't ask again": the app stops asking the file host at all. */
  async stop() {
    await settings.update({ ask_for_annotations: false });
    this.stopped = true;
  }

  /** Closes it; one being fetched carries on, and says when it's done. */
  close() {
    this.shown = null;
    this.stopped = false;
    this.error = null;
  }
}

export const offers = new Offers();
