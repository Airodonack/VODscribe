<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, events, type SetupStatus, type UnlistenFn } from "./api";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";

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
  <div class="brand">
    <Logo size={26} tile />
    <span class="name">VODscribe</span>
  </div>
  <h1>A couple of downloads, then you're set</h1>
  <p class="muted lead">
    Everything runs on this computer: the audio, the transcripts and the speech model stay here.
  </p>

  <ul class="card">
    <li>
      <span class="check" class:ok={status.model} class:busy={running && !status.model}>
        {#if status.model}<Icon name="check" size={14} />{:else if running}<span class="spin"></span>{:else}1{/if}
      </span>
      <div>
        <strong>Speech model</strong>
        <span class="muted">Parakeet TDT 0.6B v3, about 900 MB</span>
        {#if running && !status.model}
          <div class="bar"><div style:width={pct(progress.model)}></div></div>
        {/if}
      </div>
    </li>
    <li>
      <span class="check" class:ok={status.yt_dlp} class:busy={running && !status.yt_dlp}>
        {#if status.yt_dlp}<Icon name="check" size={14} />{:else if running}<span class="spin"></span>{:else}2{/if}
      </span>
      <div>
        <strong>yt-dlp</strong>
        <span class="muted">fetches audio from Kick, Twitch and YouTube</span>
        {#if running && !status.yt_dlp}
          <div class="bar"><div style:width={pct(progress["yt-dlp"])}></div></div>
        {/if}
      </div>
    </li>
    <li>
      <span class="check" class:ok={status.ffmpeg} class:bad={!status.ffmpeg}>
        {#if status.ffmpeg}<Icon name="check" size={14} />{:else}<Icon name="x" size={14} />{/if}
      </span>
      <div>
        <strong>ffmpeg</strong>
        <span class="muted">decodes audio</span>
        {#if !status.ffmpeg}
          <p class="detail">
            Not found. Install it, then check again:
            <code>sudo apt install ffmpeg</code>, <code>brew install ffmpeg</code> or <code>winget install ffmpeg</code>.
          </p>
        {/if}
      </div>
    </li>
    <li>
      <span class="check" class:ok={status.js_runtime}>
        {#if status.js_runtime}<Icon name="check" size={14} />{:else}–{/if}
      </span>
      <div>
        <strong>JavaScript runtime</strong>
        <span class="muted">optional; makes YouTube downloads reliable</span>
        {#if !status.js_runtime}
          <p class="detail muted">Install Deno or Node.js if YouTube links fail.</p>
        {/if}
      </div>
    </li>
  </ul>

  {#if error}
    <p class="error-text"><Icon name="error" size={14} /> {error}</p>
  {/if}

  <div class="actions">
    {#if !status.model || !status.yt_dlp}
      <button class="primary" onclick={start} disabled={running}>
        {#if running}Downloading…{:else}<Icon name="download" size={15} /> Download{/if}
      </button>
    {/if}
    <button onclick={onchange} disabled={running}>Check again</button>
  </div>
</div>

<style>
  .setup {
    max-width: 540px;
    margin: 9vh auto;
    padding: 0 24px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 22px;
  }


  .name {
    font-weight: 700;
    font-size: 15px;
  }

  h1 {
    margin: 0 0 6px;
    font-size: 22px;
    line-height: 1.25;
    letter-spacing: -0.01em;
  }

  .lead {
    margin: 0 0 20px;
  }

  .card {
    list-style: none;
    padding: 0;
    margin: 0 0 20px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
  }

  li {
    display: flex;
    gap: 14px;
    padding: 12px 14px;
  }

  li + li {
    border-top: 1px solid var(--border);
  }

  li > div {
    flex: 1;
    min-width: 0;
  }

  li strong {
    margin-right: 6px;
  }

  .detail {
    margin: 4px 0 0;
    font-size: 13px;
  }

  .check {
    flex: none;
    width: 24px;
    height: 24px;
    margin-top: -1px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 12px;
    font-weight: 700;
  }

  .check.ok {
    background: var(--ok);
    color: #fff;
  }

  .check.bad {
    background: var(--danger-soft);
    color: var(--danger);
  }

  .check.busy {
    background: var(--accent-soft);
  }

  .spin {
    width: 12px;
    height: 12px;
    border: 2px solid var(--accent);
    border-right-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .bar {
    height: 4px;
    margin-top: 8px;
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

  .error-text {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  code {
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--surface-2);
  }
</style>
