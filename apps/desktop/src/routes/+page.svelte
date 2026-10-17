<script lang="ts">
  import "../app.css";
  import App from "#lib/App.svelte";
  import LibraryProblem from "#lib/LibraryProblem.svelte";
  import { api } from "#lib/api.ts";

  // Whether the library opened; if it didn't, the recovery screen instead of
  // the app, which needs it. Asked once, before anything else.
  const problem = api.libraryProblem().catch(() => null);
</script>

<svelte:head>
  <title>VODscribe</title>
</svelte:head>

{#await problem then error}
  {#if error}
    <LibraryProblem {error} />
  {:else}
    <App />
  {/if}
{/await}
