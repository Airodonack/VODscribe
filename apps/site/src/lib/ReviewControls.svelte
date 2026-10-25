<script lang="ts">
  // A chapter's or highlight's review, under its row in the annotations pane
  // (review.svelte.ts): keep, wrong or misplaced (a second click takes the
  // verdict back), where a misplaced one should start (the playhead's time
  // when it's picked, to type over or take again), and a note.
  import { cuts } from "desktop/lib/cuts.svelte.ts";
  import { formatFraction } from "desktop/lib/platform.ts";
  import type { Review } from "#lib/review.svelte.ts";
  import { VERDICTS, parseTime, type ReviewRow, type Verdict } from "#lib/verdicts.ts";

  let { review, row }: { review: Review; row: ReviewRow } = $props();

  const judgment = $derived(review.of(row));
  // A tenth of a second is as near as a start needs saying.
  const playhead = () => Math.round(cuts.now * 10) / 10;

  function pick(verdict: Verdict) {
    if (judgment.verdict === verdict) return review.set(row, { verdict: null });
    review.set(row, {
      verdict,
      corrected_start: verdict === "misplaced" ? playhead() : judgment.corrected_start,
    });
  }

  let badTime = $state(false);
</script>

<div class="review-controls">
  <div class="verdicts" role="group" aria-label="Verdict on “{row.title}”">
    {#each VERDICTS as v (v.value)}
      <button
        class="verdict {v.value}"
        class:on={judgment.verdict === v.value}
        aria-pressed={judgment.verdict === v.value}
        title={v.hint}
        onclick={() => pick(v.value)}>{v.label}</button
      >
    {/each}
  </div>
  {#if judgment.verdict === "misplaced"}
    <div class="at">
      <label>
        <span class="muted">Should start at</span>
        <input
          type="text"
          class:error={badTime}
          value={judgment.corrected_start === null ? "" : formatFraction(judgment.corrected_start, 1)}
          onchange={(e) => {
            const t = parseTime(e.currentTarget.value);
            badTime = t === null && e.currentTarget.value.trim() !== "";
            if (!badTime) review.set(row, { corrected_start: t });
          }}
          aria-invalid={badTime}
          title="A time such as 1:02:05 or 12.5"
          spellcheck="false"
        />
      </label>
      <button
        class="link"
        onclick={() => {
          badTime = false;
          review.set(row, { corrected_start: playhead() });
        }}
        title="Take the playhead's time">Playhead</button
      >
    </div>
  {/if}
  <input
    type="text"
    class="note"
    placeholder="Note"
    aria-label="Note on “{row.title}”"
    value={judgment.note}
    oninput={(e) => review.set(row, { note: e.currentTarget.value })}
  />
</div>

<style>
  .review-controls {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .verdicts {
    display: flex;
    gap: 4px;
  }

  .verdict {
    flex: 1;
    padding: 2px 6px;
    font-size: 12px;
    font-weight: 500;
  }

  .verdict.keep.on {
    background: var(--ok);
    border-color: var(--ok);
    color: #fff;
  }

  .verdict.wrong.on {
    background: var(--danger);
    border-color: var(--danger);
    color: #fff;
  }

  .verdict.misplaced.on {
    background: var(--warn);
    border-color: var(--warn);
    color: #fff;
  }

  .at {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }

  .at label {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .at input {
    width: 7em;
    padding: 2px 6px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .at input.error {
    border-color: var(--danger);
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font-size: 12px;
    font-weight: 400;
  }

  .note {
    padding: 3px 8px;
    font-size: 12px;
  }
</style>
