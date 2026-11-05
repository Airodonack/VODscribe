<script lang="ts">
  // The mark: a papyrus roll with a play triangle cut from its face, or for
  // VODscriptions (`mark="vodscriptions"`) the same roll with a five-point
  // star, the highlight's mark, cut from it in place of the triangle.
  // `tile` draws it white on the app-icon gradient.
  let {
    size = 18,
    tile = false,
    mark = "vodscribe",
  }: { size?: number; tile?: boolean; mark?: "vodscribe" | "vodscriptions" } = $props();

  const ROLL = "M12 18 h40 a6 6 0 0 1 6 6 v16 a6 6 0 0 1 -6 6 h-40 a6 6 0 0 1 -6 -6 v-16 a6 6 0 0 1 6 -6 z";
  const PLAY = "M27 24 v16 l13 -8 z";
  // Centred on the face as its outline is (the points reach 12 from the
  // middle, the dents 5.2), wide enough to read on an 18 px tile.
  const STAR =
    "M32 21.15 L35.06 28.94 L43.41 29.44 L36.95 34.75 L39.05 42.85 L32 38.35 L24.95 42.85 L27.05 34.75 L20.59 29.44 L28.94 28.94 z";
  const face = $derived(`${ROLL} ${mark === "vodscriptions" ? STAR : PLAY}`);
</script>

{#if tile}
  <span class="tile" style:width="{size}px" style:height="{size}px" style:border-radius="{size * 0.22}px">
    <svg width={size * 0.72} height={size * 0.72} viewBox="0 0 64 64" fill="#fff" aria-hidden="true">
      <path fill-rule="evenodd" d={face} />
      <path d="M6 14 a6 6 0 0 1 6 6 v24 a6 6 0 0 1 -6 6 a6 6 0 0 1 -4 -6 v-24 a6 6 0 0 1 4 -6 z" />
      <path d="M58 14 a6 6 0 0 1 4 6 v24 a6 6 0 0 1 -4 6 a6 6 0 0 1 -6 -6 v-24 a6 6 0 0 1 6 -6 z" />
    </svg>
  </span>
{:else}
  <svg width={size} height={size} viewBox="0 0 64 64" fill="currentColor" aria-hidden="true">
    <path fill-rule="evenodd" d={face} />
    <path d="M6 14 a6 6 0 0 1 6 6 v24 a6 6 0 0 1 -6 6 a6 6 0 0 1 -4 -6 v-24 a6 6 0 0 1 4 -6 z" />
    <path d="M58 14 a6 6 0 0 1 4 6 v24 a6 6 0 0 1 -4 6 a6 6 0 0 1 -6 -6 v-24 a6 6 0 0 1 6 -6 z" />
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
