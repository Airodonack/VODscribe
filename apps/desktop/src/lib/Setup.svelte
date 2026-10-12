<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, events, type DownloadProgress, type SetupStatus, type UnlistenFn } from "./api";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import DownloadStep from "./DownloadStep.svelte";
  import { formatBytes, os } from "./platform";
  import Tip from "./Tip.svelte";

  let { status, onchange }: { status: SetupStatus; onchange: () => void } = $props();

  // The item under way and its step, named like the Settings downloads:
  // "Downloading <exact name>", then "<name> matches its SHA-256".
  let current = $state<string | null>(null);
  let step = $state<DownloadProgress | null>(null);
  let error = $state<string | null>(null);
  let running = $derived(status.running);
  const unlisten: UnlistenFn[] = [];
  // Only the install steps for this computer are shown (os, from platform.ts).

  onMount(async () => {
    unlisten.push(
      await events.setupProgress((item, progress) => {
        // A new item means the last one finished: refresh so it shows as done and checked.
        if (current && current !== item) onchange();
        current = item;
        step = progress;
      }),
      await events.setupDone((e) => {
        error = e;
        onchange();
      }),
    );
  });
  onDestroy(() => unlisten.forEach((u) => u()));

  const CHECKED =
    "VODscribe checked that this is exactly the file that was published, so it wasn't damaged or tampered with.";
  let items = $derived([
    {
      key: "yt-dlp",
      name: "Video downloader",
      what: "gets the audio from Kick, Twitch and YouTube",
      page: "https://github.com/yt-dlp/yt-dlp",
      checked: "VODscribe checked it against its makers' official signature, so it wasn't damaged or tampered with.",
      done: status.yt_dlp,
      managed: status.yt_dlp_managed,
      downloads: true,
      optional: false,
    },
    {
      key: "ffmpeg",
      name: "Video reader",
      what: "opens videos so VODscribe can hear them",
      page: os === "mac" ? "https://ffmpeg.martin-riedl.de" : "https://github.com/BtbN/FFmpeg-Builds",
      checked: CHECKED,
      done: status.ffmpeg,
      managed: status.ffmpeg_managed,
      downloads: status.can_install_tools,
      optional: false,
    },
    {
      key: "model",
      name: "Speech recognition",
      what: `turns speech into text, about ${formatBytes(status.model_bytes)}`,
      page: "https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3",
      checked: CHECKED,
      done: status.model,
      managed: true,
      downloads: true,
      optional: false,
    },
    {
      key: "deno",
      name: "YouTube helper",
      what: "some YouTube VODs need it",
      page: "https://github.com/denoland/deno",
      checked: CHECKED,
      done: status.js_runtime,
      managed: status.js_runtime_managed,
      downloads: status.can_install_tools,
      optional: true,
    },
  ]);

  async function start() {
    error = null;
    step = null;
    current = null;
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
    {#each items as item, i (item.key)}
      <!-- Downloads run one at a time: only the one under way spins. -->
      {@const busy = running && !item.done && item.downloads && current === item.key}
      <li>
        <span class="check" class:ok={item.done} class:busy class:bad={!item.done && !item.downloads && !item.optional}>
          {#if item.done}<Icon name="check" size={14} />{:else if busy}<span class="spin"
            ></span>{:else if !item.downloads && !item.optional}<Icon name="x" size={14} />{:else}{i + 1}{/if}
        </span>
        <div>
          <!-- The plain name for everyone; it links to where the part comes from. -->
          <a
            class="name-link"
            href={item.page}
            title={item.page}
            onclick={(e) => (e.preventDefault(), openUrl(item.page))}><strong>{item.name}</strong></a
          >
          <span class="muted">{item.what}</span>
          {#if item.done && item.managed}
            <p class="verified">
              <Tip text={item.checked}><Icon name="shield" size={12} /> Downloaded and checked</Tip>
            </p>
          {:else if item.done}
            <p class="detail muted">Using the one already installed on this computer.</p>
          {:else if busy}
            <div class="progress">
              <!-- The exact component and its checks, as in Settings. -->
              <DownloadStep progress={step} />
            </div>
          {:else if item.key === "ffmpeg" && !item.downloads}
            <p class="detail">
              Not installed yet.
              {#if os === "windows"}
                Open <strong>Terminal</strong> from the Start menu, type <code>winget install ffmpeg</code> and press Enter.
              {:else if os === "mac"}
                Open <strong>Terminal</strong>, type <code>brew install ffmpeg</code> and press Enter. (This uses
                <a href="https://brew.sh" onclick={(e) => (e.preventDefault(), openUrl("https://brew.sh"))}>Homebrew</a
                >.)
              {:else}
                Install <strong>ffmpeg</strong> from your software center, or run <code>sudo apt install ffmpeg</code>.
              {/if}
              Then close and reopen VODscribe.
            </p>
          {/if}
        </div>
      </li>
    {/each}
  </ul>

  {#if error}
    <p class="error-text"><Icon name="error" size={14} /> {error}</p>
  {/if}

  <div class="actions">
    {#if items.some((item) => !item.done && item.downloads)}
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

  .progress {
    display: grid;
    gap: 5px;
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

  .name-link {
    color: inherit;
    text-decoration: underline var(--border);
    text-underline-offset: 3px;
  }

  .name-link:hover {
    color: var(--accent);
    text-decoration-color: currentColor;
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
