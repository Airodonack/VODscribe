<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, events, type SetupStatus, type UnlistenFn } from "./api";

  let { status, onchange }: { status: SetupStatus; onchange: () => void } = $props();

  let progress = $state<Record<string, number>>({});
  let error = $state<string | null>(null);
  let running = $derived(status.running);
  const unlisten: UnlistenFn[] = [];

  onMount(async () => {
    unlisten.push(
      await events.setupProgress((step, done, total) => {
        progress[step] = total ? done / total : 0;
      }),
      await events.setupDone((e) => {
        error = e;
        onchange();
      }),
    );
  });
  onDestroy(() => unlisten.forEach((u) => u()));

  async function start() {
    error = null;
    await api.runSetup();
    onchange();
  }

  const pct = (f: number | undefined) => `${Math.round((f ?? 0) * 100)}%`;
</script>

<div class="setup">
  <h1>Set up vodscribe</h1>
  <p class="muted">Everything runs on this computer. Two downloads are needed once.</p>

  <ul>
    <li>
      <span class="check" class:ok={status.model}>{status.model ? "✓" : "•"}</span>
      <div>
        <strong>Speech model</strong> <span class="muted">Parakeet TDT 0.6B v3, about 900 MB</span>
        {#if running && !status.model}
          <div class="bar"><div style:width={pct(progress.model)}></div></div>
        {/if}
      </div>
    </li>
    <li>
      <span class="check" class:ok={status.yt_dlp}>{status.yt_dlp ? "✓" : "•"}</span>
      <div>
        <strong>yt-dlp</strong> <span class="muted">fetches audio from Kick, Twitch and YouTube</span>
        {#if running && !status.yt_dlp}
          <div class="bar"><div style:width={pct(progress["yt-dlp"])}></div></div>
        {/if}
      </div>
    </li>
    <li>
      <span class="check" class:ok={status.ffmpeg}>{status.ffmpeg ? "✓" : "✗"}</span>
      <div>
        <strong>ffmpeg</strong> <span class="muted">decodes audio</span>
        {#if !status.ffmpeg}
          <p class="error-text">
            Not found. Install it (for example <code>sudo apt install ffmpeg</code>, <code>brew install ffmpeg</code>
            or <code>winget install ffmpeg</code>), then check again.
          </p>
        {/if}
      </div>
    </li>
    <li>
      <span class="check" class:ok={status.js_runtime}>{status.js_runtime ? "✓" : "–"}</span>
      <div>
        <strong>JavaScript runtime</strong> <span class="muted">optional; makes YouTube downloads reliable</span>
        {#if !status.js_runtime}
          <p class="muted">Install Deno or Node.js if YouTube links fail.</p>
        {/if}
      </div>
    </li>
  </ul>

  {#if error}
    <p class="error-text">{error}</p>
  {/if}

  <div class="actions">
    {#if !status.model || !status.yt_dlp}
      <button class="primary" onclick={start} disabled={running}>
        {running ? "Downloading…" : "Download"}
      </button>
    {/if}
    <button onclick={onchange} disabled={running}>Check again</button>
  </div>
</div>

<style>
  .setup {
    max-width: 520px;
    margin: 10vh auto;
    padding: 0 24px;
  }

  h1 {
    margin: 0 0 4px;
    font-size: 22px;
  }

  ul {
    list-style: none;
    padding: 0;
    margin: 24px 0;
    display: grid;
    gap: 16px;
  }

  li {
    display: flex;
    gap: 12px;
  }

  li > div {
    flex: 1;
  }

  li p {
    margin: 4px 0 0;
    font-size: 13px;
  }

  .check {
    width: 20px;
    text-align: center;
    color: var(--muted);
    font-weight: 700;
  }

  .check.ok {
    color: var(--ok);
  }

  .bar {
    height: 4px;
    margin-top: 6px;
    border-radius: 2px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .bar > div {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s;
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  code {
    font-family: var(--mono);
    font-size: 12px;
  }
</style>
