<script lang="ts">
  // A small turning ring, for work under way with no end in sight yet.
  let { size = 12, label }: { size?: number; label?: string } = $props();
</script>

<span
  class="spinner"
  style:width="{size}px"
  style:height="{size}px"
  role={label ? "status" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}
></span>

<style>
  .spinner {
    display: inline-block;
    flex: none;
    vertical-align: -0.1em;
    border: 2px solid color-mix(in srgb, currentColor 22%, transparent);
    border-top-color: currentColor;
    border-radius: 50%;
    animation: turn 0.8s linear infinite;
  }

  @keyframes turn {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation-duration: 2.4s;
    }
  }
</style>
