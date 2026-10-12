<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, events, type SetupStatus, type UnlistenFn } from "./api";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import Progress from "./Progress.svelte";

  let { status, onchange }: { status: SetupStatus; onchange: () => void } = $props();

  let progress = $state<Record<string, number>>({});
  let error = $state<string | null>(null);
  let running = $derived(status.running);
  const unlisten: UnlistenFn[] = [];
  // Only the install steps for this computer are shown.
  const os = /Win/.test(navigator.platform) ? "windows" : /Mac/.test(navigator.platform) ? "mac" : "linux";

  onMount(async () => {
    unlisten.push(
      await events.setupProgress((step, done, total) => {
        const wasDone = (progress[step] ?? 0) >= 1;
        progress[step] = total ? done / total : 0;
        // Refresh as each download finishes, so it shows as installed and verified.
        if (!wasDone && progress[step] >= 1) setTimeout(onchange, 300);
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
</script>

<div class="setup">
  <div class="brand">
    <Logo size={26} tile />
    <span class="name">VODscribe</span>
  </div>
  <h1>A couple of downloads, then you're set</h1>
  <p class="muted lead">VODscribe works on your computer, not in the cloud, so your VODs and transcripts stay here.</p>

  <ul class="card">
    <li>
      <span class="check" class:ok={status.model} class:busy={running && !status.model}>
        {#if status.model}<Icon name="check" size={14} />{:else if running}<span class="spin"></span>{:else}1{/if}
      </span>
      <div>
        <strong>Speech recognition</strong>
        <span class="muted">turns speech into text, about 900 MB</span>
        {#if status.model}
          <p
            class="verified"
            title="VODscribe checked that this is exactly the file that was published, so it wasn't damaged or tampered with."
          >
            <Icon name="shield" size={12} /> Downloaded and checked
          </p>
        {/if}
        {#if running && !status.model}
          <div class="bar"><Progress value={progress.model ?? 0} /></div>
        {/if}
      </div>
    </li>
    <li>
      <span class="check" class:ok={status.yt_dlp} class:busy={running && !status.yt_dlp}>
        {#if status.yt_dlp}<Icon name="check" size={14} />{:else if running}<span class="spin"></span>{:else}2{/if}
      </span>
      <div>
        <strong>Video downloader</strong>
        <span class="muted">gets the audio from Kick, Twitch and YouTube</span>
        {#if status.yt_dlp_managed}
          <p
            class="verified"
            title="VODscribe checked it against its makers' official signature, so it wasn't damaged or tampered with."
          >
            <Icon name="shield" size={12} /> Downloaded and checked
          </p>
        {:else if status.yt_dlp}
          <p class="detail muted">Using the one already installed on this computer.</p>
        {/if}
        {#if running && !status.yt_dlp}
          <div class="bar"><Progress value={progress["yt-dlp"] ?? 0} /></div>
        {/if}
      </div>
    </li>
    <li>
      <span class="check" class:ok={status.ffmpeg} class:bad={!status.ffmpeg}>
        {#if status.ffmpeg}<Icon name="check" size={14} />{:else}<Icon name="x" size={14} />{/if}
      </span>
      <div>
        <strong>ffmpeg</strong>
        <span class="muted">a free tool VODscribe uses to read audio</span>
        {#if !status.ffmpeg}
          <p class="detail">
            Not installed yet.
            {#if os === "windows"}
              Open <strong>Terminal</strong> from the Start menu, type <code>winget install ffmpeg</code> and press Enter.
            {:else if os === "mac"}
              Open <strong>Terminal</strong>, type <code>brew install ffmpeg</code> and press Enter. (This uses
              <a href="https://brew.sh" onclick={(e) => (e.preventDefault(), openUrl("https://brew.sh"))}>Homebrew</a>.)
            {:else}
              Install <strong>ffmpeg</strong> from your software center, or run <code>sudo apt install ffmpeg</code>.
            {/if}
            Then close and reopen VODscribe.
          </p>
        {/if}
      </div>
    </li>
    <li>
      <span class="check" class:ok={status.js_runtime}>
        {#if status.js_runtime}<Icon name="check" size={14} />{:else}–{/if}
      </span>
      <div>
        <strong>YouTube helper</strong>
        <span class="muted">optional; some YouTube VODs need it</span>
        {#if !status.js_runtime}
          <p class="detail muted">
            If YouTube links fail to add, install
            <a href="https://deno.com" onclick={(e) => (e.preventDefault(), openUrl("https://deno.com"))}>Deno</a> (free),
            then reopen VODscribe.
          </p>
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

  .verified {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 3px 0 0;
    font-size: 12px;
    color: var(--ok);
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
    margin-top: 8px;
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

  .detail a {
    color: var(--accent);
  }

  code {
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--surface-2);
  }
</style>
