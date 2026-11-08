<script lang="ts">
  // The note at a drop on DaVinci Resolve on Linux (resolveDrop.svelte.ts):
  // Switch to MOV (unless the ffmpeg in use can't make it: then the note
  // says why), or Not now; and Don't ask again.
  import Modal from "./Modal.svelte";
  import { WHERE_UNDONE, noteText, resolveNote } from "./resolveDrop.svelte";

  let open = $state(false);
  let hide = $state(false);
  $effect(() => {
    const up = resolveNote.shown !== null;
    open = up;
    if (up) hide = false;
  });
  // Closed by Escape, the close button or the backdrop: Not now.
  $effect(() => {
    if (!open && resolveNote.shown) void resolveNote.answer(false, hide);
  });

  const shown = $derived(resolveNote.shown);
</script>

<Modal bind:open label="DaVinci Resolve" small>
  {#if shown}
    <div class="note">
      <p>{noteText(shown.how, shown.count, shown.what)}{resolveNote.cantSwitch ? ` ${resolveNote.cantSwitch}` : ""}</p>
      <label><input type="checkbox" bind:checked={hide} /> Don't ask again</label>
      {#if hide}<p class="muted">{WHERE_UNDONE}</p>{/if}
      <div class="buttons">
        <button onclick={() => resolveNote.answer(false, hide)}>Not now</button>
        {#if !resolveNote.cantSwitch}
          <button class="primary" data-autofocus onclick={() => resolveNote.answer(true, hide)}>Switch to MOV</button>
        {/if}
      </div>
    </div>
  {/if}
</Modal>

<style>
  .note {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-size: 13px;
  }

  p {
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
