<script lang="ts">
  import { thumbUrl } from "./api";

  // A VOD's local thumbnail, covering its container. It renders nothing until it
  // has loaded, or if it can't, so whatever is underneath shows instead.
  let { name }: { name: string } = $props();
  let failed = $state(false);
</script>

{#await thumbUrl(name) then src}
  {#if !failed}
    <img {src} alt="" draggable="false" onerror={() => (failed = true)} />
  {/if}
{/await}

<style>
  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
</style>
