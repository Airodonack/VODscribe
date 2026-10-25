<script lang="ts">
  import Modal from "./Modal.svelte";
  import { offers } from "./offers.svelte";
  import { settingsWindow } from "./settingsWindow.svelte";

  // The offer of a VOD's published annotations (offers.svelte.ts): Use, Not
  // now, and after an add Don't ask again. Already brought in and unchanged,
  // it says so and offers to replace them anyway.

  let open = $state(false);
  $effect(() => {
    open = offers.shown !== null;
  });
  // Closed by Escape, the close button or the backdrop: Not now.
  $effect(() => {
    if (!open && offers.shown) offers.close();
  });

  const shown = $derived(offers.shown);
</script>

<Modal bind:open label="Transcript and annotations" small>
  {#if shown}
    <div class="offer">
      {#if offers.stopped}
        <p>VODscribe won't ask about the VODs you add any more.</p>
        <p class="muted">You can turn it back on in Settings › Privacy.</p>
        <div class="buttons">
          <button
            onclick={() => {
              offers.close();
              settingsWindow.show("privacy");
            }}>Open Settings</button
          >
          <button class="primary" data-autofocus onclick={() => offers.close()}>OK</button>
        </div>
      {:else}
        <p class="title">{shown.vod.title}</p>
        {#if shown.offer.up_to_date}
          <p>This VOD has the latest transcript and annotations already.</p>
        {:else}
          <p>A transcript and annotations are available for this VOD.</p>
        {/if}
        {#if shown.offer.local_transcript && !shown.offer.up_to_date}
          <p class="warn-text">This replaces the transcript made on this computer.</p>
        {/if}
        <p class="muted small">
          Chapters and highlights come with them; the ones you made stay.
          {#if shown.afterAdd}To find them, VODscribe sent only the VOD's site and id.{/if}
        </p>
        {#if offers.error}<p class="error-text">{offers.error}</p>{/if}
        <div class="buttons">
          {#if shown.afterAdd}
            <button class="link" onclick={() => offers.stop()} disabled={offers.busy}>Don't ask again</button>
          {/if}
          <span class="space"></span>
          <button onclick={() => offers.close()} disabled={offers.busy}>Not now</button>
          <button class="primary" data-autofocus onclick={() => offers.use()} disabled={offers.busy}>
            {offers.busy ? "Getting them…" : shown.offer.up_to_date ? "Replace anyway" : "Use"}
          </button>
        </div>
      {/if}
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

  .buttons {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
  }

  .space {
    flex: 1;
  }
</style>
