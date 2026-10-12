<script lang="ts">
  // Shown at the top of the library when a newer version is out.
  import Icon from "./Icon.svelte";
  import Progress from "./Progress.svelte";
  import { updates } from "./updates.svelte";
</script>

{#if updates.available}
  <div class="banner">
    {#if updates.installing}
      <span>Updating to {updates.available.version}…</span>
      <Progress value={updates.progress ?? 0} indeterminate={updates.progress === null} thin />
    {:else}
      <span><Icon name="download" size={14} /> VODscribe {updates.available.version} is out.</span>
      <button class="primary" onclick={() => updates.install()}>Update and restart</button>
    {/if}
    {#if updates.error}<p class="error-text">{updates.error}</p>{/if}
  </div>
{/if}

<style>
  .banner {
    display: grid;
    gap: 8px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--accent-soft);
    font-size: 13px;
  }

  .banner span {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
  }

  .banner button {
    justify-self: start;
    padding: 4px 12px;
    font-size: 13px;
  }

  .error-text {
    margin: 0;
    font-weight: 400;
  }
</style>
