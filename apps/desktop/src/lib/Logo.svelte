<script lang="ts">
  // The mark: a seated scribe in the manner of an Egyptian hieroglyph, with a
  // papyrus roll across the lap. `tile` draws it white on the app-icon gradient.
  let { size = 18, tile = false }: { size?: number; tile?: boolean } = $props();
</script>

{#if tile}
  <span class="tile" style:width="{size}px" style:height="{size}px" style:border-radius="{size * 0.22}px">
    <svg width={size * 0.72} height={size * 0.72} viewBox="0 0 64 64" fill="#fff" aria-hidden="true">
      <circle cx="33" cy="16" r="7.5" />
      <path d="M36 9 h7 a6 6 0 0 1 6 6 v11 a2 2 0 0 1 -2 2 h-11 z" />
      <path d="M31 27 h18 l4 21 h-20 z" />
      <path d="M31 28 l5 3 l-14 13 l-5 -3 z" />
      <path
        fill-rule="evenodd"
        d="M9 39 h26 a3.5 3.5 0 0 1 3.5 3.5 v2 a3.5 3.5 0 0 1 -3.5 3.5 h-26 a3.5 3.5 0 0 1 -3.5 -3.5 v-2 a3.5 3.5 0 0 1 3.5 -3.5 z M10 42 h24 v3 h-24 z"
      />
      <path d="M10 47 h46 a5 5 0 0 1 5 5 v2 a4 4 0 0 1 -4 4 h-50 a4 4 0 0 1 -4 -4 v-2 a5 5 0 0 1 5 -5 z" />
      <circle cx="11" cy="50" r="6" />
    </svg>
  </span>
{:else}
  <svg width={size} height={size} viewBox="0 0 64 64" fill="currentColor" aria-hidden="true">
    <circle cx="33" cy="16" r="7.5" />
    <path d="M36 9 h7 a6 6 0 0 1 6 6 v11 a2 2 0 0 1 -2 2 h-11 z" />
    <path d="M31 27 h18 l4 21 h-20 z" />
    <path d="M31 28 l5 3 l-14 13 l-5 -3 z" />
    <path
      fill-rule="evenodd"
      d="M9 39 h26 a3.5 3.5 0 0 1 3.5 3.5 v2 a3.5 3.5 0 0 1 -3.5 3.5 h-26 a3.5 3.5 0 0 1 -3.5 -3.5 v-2 a3.5 3.5 0 0 1 3.5 -3.5 z M10 42 h24 v3 h-24 z"
    />
    <path d="M10 47 h46 a5 5 0 0 1 5 5 v2 a4 4 0 0 1 -4 4 h-50 a4 4 0 0 1 -4 -4 v-2 a5 5 0 0 1 5 -5 z" />
    <circle cx="11" cy="50" r="6" />
  </svg>
{/if}

<style>
  svg {
    display: block;
    flex: none;
  }

  .tile {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, #4f46e5, #a78bfa);
  }
</style>
