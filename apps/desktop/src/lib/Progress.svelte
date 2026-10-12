<script lang="ts">
  // A progress bar. `value` is 0..1; `paused` greys the fill; `thin` is for
  // dense lists such as the library; `indeterminate` slides while the total is unknown.
  let {
    value,
    paused = false,
    thin = false,
    indeterminate = false,
  }: { value: number; paused?: boolean; thin?: boolean; indeterminate?: boolean } = $props();
</script>

<span
  class="bar"
  class:thin
  role="progressbar"
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={indeterminate ? undefined : Math.round(value * 100)}
>
  <span
    class="fill"
    class:paused
    class:indeterminate
    style:width={indeterminate ? null : `${Math.min(1, Math.max(0, value)) * 100}%`}
  ></span>
</span>

<style>
  .bar {
    display: block;
    width: 100%;
    height: 4px;
    border-radius: 2px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .bar.thin {
    height: 3px;
  }

  .fill {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.25s;
  }

  .fill.indeterminate {
    width: 30%;
    animation: slide 1.2s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  .fill.paused {
    background: var(--muted);
  }
</style>
