<script lang="ts">
  // A VOD with annotations: its files from the download host, or ?src=<url> (VodView.svelte).
  import { page } from "$app/state";
  import VodView from "#lib/VodView.svelte";
  import { pickSource } from "#lib/tauri/annotated.ts";

  const source = $derived(
    pickSource(page.params.platform ?? "", page.params.id ?? "", page.url.searchParams, page.url.href),
  );
</script>

<!-- Afresh for each address: a link from one VOD to another keeps this page. -->
{#key page.url.href}
  <VodView {source} />
{/key}
