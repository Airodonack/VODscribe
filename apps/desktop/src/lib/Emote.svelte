<script lang="ts">
  import { emoteUrl } from "./api";

  let { name, url }: { name: string; url: string } = $props();

  // Until the image is in (or if it can't be had), the emote reads as its name.
  let src = $state<string | null>(null);
  $effect(() => {
    let current = true;
    src = null;
    emoteUrl(url).then(
      (u) => current && (src = u),
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
