<script lang="ts">
  import Modal from "./Modal.svelte";
  import { offers } from "./offers.svelte";

  // The offer of a VOD's published annotations (offers.svelte.ts): Use or
  // Not now, and after an add "Don't ask again", as the other notes have it
  // (ResolveNote.svelte): ticked, the answer given is kept (Use: used
  // unasked from now on; Not now, Escape or closing: not offered). Already brought in
  // and unchanged, it says so and offers to replace them anyway.

  // Either answer can be kept, so where it's changed, not "turned back on".
  const WHERE_CHANGED = "You can change this in Settings › Privacy.";

  let open = $state(false);
  let hide = $state(false);
  $effect(() => {
    open = offers.shown !== null;
  });
  // A new offer starts with the box clear.
  $effect(() => {
    if (offers.shown) hide = false;
  });
  // Closed by Escape, the close button or the backdrop: Not now.
  $effect(() => {
    if (!open && offers.shown) offers.close(hide);
  });

  const shown = $derived(offers.shown);
</script>

<Modal bind:open label="VODscription found" small>
  {#if shown}
    <div class="offer">
      <p class="title">{shown.vod.title}</p>
      {#if shown.offer.up_to_date && shown.unasked}
        <p>When last checked, this VOD had the latest VODscription.</p>
      {:else if shown.offer.up_to_date}
        <p>This VOD has the latest VODscription already.</p>
      {:else if shown.offer.newer}
        <p>A newer VODscription is available for this VOD.</p>
      {:else}
        <p>A VODscription is available for this VOD: a transcript, chapters and highlights.</p>
      {/if}
      {#if shown.offer.local_transcript && !shown.offer.up_to_date}
        <p class="warn-text">This replaces the transcript made on this computer.</p>
      {/if}
      {#if shown.unasked}<p class="muted small">{shown.unasked}</p>{/if}
      <p class="muted small">
        The chapters and highlights you made stay.
        {#if shown.afterAdd}To find it, VODscribe sent only the VOD's site and id.{/if}
      </p>
      {#if shown.afterAdd}
        <label><input type="checkbox" bind:checked={hide} /> Don't ask again</label>
        {#if hide}<p class="muted">{WHERE_CHANGED}</p>{/if}
      {/if}
      {#if offers.error}<p class="error-text">{offers.error}</p>{/if}
      <div class="buttons">
        <button onclick={() => offers.close(hide)} disabled={offers.busy}>Not now</button>
        <button class="primary" data-autofocus onclick={() => offers.use(hide)} disabled={offers.busy}>
          {offers.busy ? "Getting it…" : shown.offer.up_to_date ? "Replace anyway" : "Use"}
        </button>
      </div>
    </div>
  {/if}
</Modal>

<style>
  .offer {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  p {
    margin: 0;
  }

  .title {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .small {
    font-size: 12px;
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
    margin-top: 8px;
  }
</style>
