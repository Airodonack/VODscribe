// What a full-page VOD view (VodView.svelte) puts in the site's one-line bar
// (+layout.svelte): a note, the way to open a file from the computer, and the
// VOD's review (Review, with Export verdicts and Clear while it's on). The
// view sets them as it opens and clears them as it goes.

import type { Review } from "./review.svelte.ts";

export const siteBar = $state<{ note: string; pick: (() => void) | null; review: Review | null }>({
  note: "",
  pick: null,
  review: null,
});
