<script lang="ts">
  import { emoteUrl, emoteUrlNow } from "./api";

  let { name, url }: { name: string; url: string } = $props();

  // Until the image is in (or if it can't be had), the emote reads as its name.
  // One shown before is there from the first frame, so its row is laid out
  // once rather than twice (name, then image).
  let fetched = $state<string | null>(null);
  let src = $derived(emoteUrlNow(url) ?? fetched);
  $effect(() => {
    let current = true;
    fetched = null;
    if (emoteUrlNow(url)) return;
    emoteUrl(url).then(
      (u) => current && (fetched = u),
      () => {},
    );
    return () => {
      current = false;
    };
  });
</script>

{#if src}<img {src} alt={name} title={name} />{:else}<span class="name">{name}</span>{/if}

<style>
  img {
    height: 1.6em;
    width: auto;
    max-width: 6em;
    vertical-align: middle;
    margin: -0.25em 0;
  }

  .name {
    color: var(--muted);
  }
</style>
