// What a full-page VOD view (VodView.svelte) puts in the site's one-line bar
// (+layout.svelte): a note, and the way to open a file from the computer. The
// view sets them as it opens and clears them as it goes.

export const siteBar = $state<{ note: string; pick: (() => void) | null }>({ note: "", pick: null });
