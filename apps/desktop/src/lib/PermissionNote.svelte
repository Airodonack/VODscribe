<script lang="ts">
  // The note before downloading from a site (a clip, Player.svelte; a whole
  // video, VodRow.svelte), in the popover each shows it in, with whatever
  // else that one asks (`children`) before its "Don't show this again".

  import type { Snippet } from "svelte";
  import { OWN_VODS_NOTE, PERMISSION_NOTE } from "./permission";

  let {
    action,
    hideLabel = "Don't show this again",
    children,
    onanswer,
  }: {
    /** The button that downloads. */
    action: string;
    hideLabel?: string;
    children?: Snippet;
    /** Whether to download, and if so whether to stop asking. */
    onanswer: (download: boolean, hide: boolean) => void;
  } = $props();

  let hide = $state(false);
</script>

<div class="permission-note">
  <p>{PERMISSION_NOTE}</p>
  <p class="muted">{OWN_VODS_NOTE}</p>
  {@render children?.()}
  <label><input type="checkbox" bind:checked={hide} /> {hideLabel}</label>
  <div class="buttons">
    <button onclick={() => onanswer(false, false)}>Cancel</button>
    <button class="primary" onclick={() => onanswer(true, hide)}>{action}</button>
  </div>
</div>

<style>
  .permission-note {
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-size: 13px;
  }

  .permission-note :global(p) {
    margin: 0;
  }

  label {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
