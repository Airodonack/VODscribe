<script lang="ts">
  import EmoteProviders from "./EmoteProviders.svelte";
  import { settings as appSettings } from "./appSettings.svelte";
  import { onDestroy, onMount } from "svelte";
  import { ask } from "./ask";
  import { api, events, type Device, type DownloadProgress, type GpuStatus, type UnlistenFn } from "./api";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Icon from "./Icon.svelte";
  import { ISSUES, LICENSES, PRIVACY } from "./links";
  import DownloadStep from "./DownloadStep.svelte";
  import { DEFAULT_HEIGHT, HEIGHTS, formatBytes, videoSize } from "./platform";
  import Tip from "./Tip.svelte";
  import { updates } from "./updates.svelte";

  let { oncleared }: { oncleared: () => void } = $props();

  let device = $derived<Device>(appSettings.value?.device ?? "cpu");
  let themeChoice = $derived(appSettings.value?.theme ?? "system");
  const THEMES = [
    { id: "system", label: "System", icon: "monitor" },
    { id: "light", label: "Light", icon: "sun" },
    { id: "dark", label: "Dark", icon: "moon" },
  ] as const;
  let cacheBytes = $state<number | null>(null);
  let mediaBytes = $state<number | null>(null);
  let videoHeight = $derived(appSettings.value?.video_height ?? DEFAULT_HEIGHT);
  let clipFormat = $derived(appSettings.value?.clip_format ?? "mp4");
  let clearing = $state(false);
  let gpu = $state<GpuStatus | null>(null);
  let step = $state<DownloadProgress | null>(null);
  let error = $state<string | null>(null);
  const unlisten: UnlistenFn[] = [];
  let timer: ReturnType<typeof setInterval> | undefined;

  // Refreshes overlap (every 5 s, and on gpu-done): only the latest asked
  // applies, so one asked before the add-on finished can't bring its bar back.
  let asked = 0;
  async function refresh() {
    const n = ++asked;
    const [status, cache, media] = await Promise.all([api.gpuStatus(), api.cacheSize(), api.mediaSize()]);
    if (n !== asked) return;
    gpu = status;
    // Opened partway through a download: start from where it is, not a blank
    // bar until the next progress event (which can be a while between files).
    if (gpu.installing && !step && gpu.progress) step = gpu.progress;
    if (!gpu.installing) step = null;
    cacheBytes = cache;
    mediaBytes = media;
  }

  let deletingVideos = $state(false);
  let videosNote = $state<string | null>(null);
  async function deleteAllVideos() {
    const ok = await ask(
      `Delete all downloaded videos${mediaBytes ? ` (${formatBytes(mediaBytes)})` : ""}? Transcripts and clips stay; ` +
        "the videos play on their sites instead. Downloads under way stop.",
      "Delete videos",
      "Keep",
    );
    if (!ok) return;
    deletingVideos = true;
    error = null;
    videosNote = null;
    try {
      const kept = await api.deleteAllVideos();
      if (kept)
        videosNote = `${kept} still being transcribed from ${kept === 1 ? "its video was" : "their videos were"} kept.`;
      mediaBytes = await api.mediaSize();
    } catch (e) {
      error = String(e);
    } finally {
      deletingVideos = false;
    }
  }

  // Every setting back as it starts; the library and everything in it stay.
  async function resetSettings() {
    const ok = await ask(
      "Put every setting back as it was when VODscribe was new? Your library, transcripts, clips, edits and videos stay.",
      "Reset",
      "Keep",
    );
    if (ok) await appSettings.reset();
  }

  async function clearCache() {
    const amount = cacheBytes ? ` ${formatBytes(cacheBytes)}` : "";
    const ok = await ask(
      `Repair VODscribe? This deletes the parts it downloaded to run${amount ? ` (${amount.trim()})` : ""}, and any ` +
        "unfinished downloads, then sets it up again. Anything in progress is paused. Your library, transcripts, " +
        "chats and kept videos stay. The GPU add-on, if you have it, is added again here.",
      "Repair",
    );
    if (!ok) return;
    clearing = true;
    error = null;
    try {
      await api.clearCache();
      oncleared();
    } catch (e) {
      error = String(e);
    } finally {
      clearing = false;
      await refresh();
    }
  }

  // The popover can close before this async setup finishes; anything set up
  // after that is torn down straight away.
  let destroyed = false;
  const keep = (u: UnlistenFn) => (destroyed ? u() : unlisten.push(u));

  onMount(async () => {
    await appSettings.load();
    keep(await events.gpuProgress((progress) => (step = progress)));
    keep(await events.ytDlpProgress((progress) => (ytDlpStep = progress)));
    keep(
      await events.gpuDone((e) => {
        step = null;
        error = e;
        refresh();
      }),
    );
    await refresh();
    // Free VRAM changes as other programs use the GPU.
    if (!destroyed) timer = setInterval(refresh, 5000);
  });
  onDestroy(() => {
    destroyed = true;
    clearInterval(timer);
    unlisten.forEach((u) => u());
  });

  let dataDir = $state<string | null>(null);
  api.dataDir().then(
    (d) => (dataDir = d),
    () => {},
  );

  async function choose(d: Device) {
    await appSettings.update({ device: d });
  }

  // Platforms change often enough that an old yt-dlp stops working.
  let updating = $state(false);
  let ytDlpStatus = $state<string | null>(null);
  // The step under way, named like the GPU add-on's: signature check, download, install.
  let ytDlpStep = $state<DownloadProgress | null>(null);
  async function updateYtDlp() {
    updating = true;
    ytDlpStatus = null;
    try {
      ytDlpStatus = await api.updateYtDlp();
    } catch (e) {
      ytDlpStatus = String(e);
    } finally {
      updating = false;
      ytDlpStep = null;
    }
  }

  async function install() {
    error = null;
    await api.installGpuPack().catch((e) => (error = String(e)));
    await refresh();
  }

  async function remove() {
    if (!(await ask("Remove the GPU add-on? VODs will be transcribed with the processor.", "Remove", "Keep"))) return;
    if (device === "gpu") await choose("cpu");
    await api.removeGpuPack().catch((e) => (error = String(e)));
    await refresh();
  }

  // Why GPU can't be chosen right now, if it can't.
  let blocker = $derived.by(() => {
    if (!gpu) return "Checking…";
    if (!gpu.supported) return "Not available on this kind of computer.";
    if (!gpu.worker) return "This version of VODscribe can't use graphics cards.";
    if (!gpu.gpu) return "No NVIDIA graphics card found.";
    if (!gpu.installed) return `Needs the GPU add-on, a one-time download of about ${formatBytes(gpu.download_bytes)}.`;
    return null;
  });
  let busy = $derived(gpu?.gpu && gpu.installed && gpu.gpu.free_mb < gpu.min_free_mb);
  let offerPack = $derived(gpu?.supported && gpu.worker && gpu.gpu);
</script>

<div class="settings">
  <section>
    <span class="eyebrow">Transcribe with</span>
    <div class="options" role="radiogroup" aria-label="Transcribe with">
      <label class="option" class:checked={device === "cpu"}>
        <input type="radio" name="device" checked={device === "cpu"} onchange={() => choose("cpu")} />
        <span class="option-text">
          <span class="option-title">Processor <span class="muted">(CPU)</span></span>
          <span class="muted">Works on any computer.</span>
        </span>
      </label>
      <label class="option" class:checked={device === "gpu"} class:disabled={blocker !== null}>
        <input
          type="radio"
          name="device"
          checked={device === "gpu"}
          disabled={blocker !== null}
          onchange={() => choose("gpu")}
        />
        <span class="option-text">
          <span class="option-title">
            Graphics card <span class="muted">(GPU)</span>
            {#if gpu?.gpu}<span class="muted detail">{gpu.gpu.name}</span>{/if}
          </span>
          <span class="muted">
            {#if blocker}{blocker}{:else}NVIDIA only. Several times faster.{/if}
          </span>
        </span>
      </label>
    </div>
    {#if busy && gpu}
      <p class="hint warn-text">
        <Icon name="warning" size={13} /> Your graphics card is busy with something else, so VODs use the processor for now.
      </p>
    {/if}

    {#if offerPack}
      <!-- Not while clearing: that holds the add-on's busy flag without downloading. -->
      {#if (step || gpu?.installing) && !clearing}
        <div class="progress">
          <!-- The exact component, for the people who chose the GPU add-on. -->
          <DownloadStep
            progress={step}
            waiting="Downloading the GPU add-on"
            oncancel={() => api.cancelGpuInstall().catch((e) => (error = String(e)))}
          />
        </div>
      {:else if !gpu?.installed}
        <button onclick={install}
          ><Icon name="download" size={14} /> Download GPU add-on (about {formatBytes(
            gpu?.download_bytes ?? 0,
          )})</button
        >
      {:else}
        <p class="verified">
          <Tip
            text="VODscribe checked that every file is exactly what was published, so nothing was damaged or tampered with."
          >
            <Icon name="shield" size={12} /> GPU add-on installed and checked
          </Tip>
        </p>
        <button class="link" onclick={remove}>Remove GPU add-on</button>
      {/if}
    {/if}
    {#if error}<p class="error-text hint">{error}</p>{/if}
    <p class="muted hint">Changes apply from the next VOD.</p>
  </section>

  <section>
    <span class="eyebrow">Appearance</span>
    <p class="label">Theme</p>
    <div class="segmented" role="radiogroup" aria-label="Theme">
      {#each THEMES as t (t.id)}
        <button
          role="radio"
          aria-checked={themeChoice === t.id}
          class:on={themeChoice === t.id}
          onclick={() => appSettings.update({ theme: t.id })}
        >
          <Icon name={t.icon} size={14} />
          {t.label}
        </button>
      {/each}
    </div>
    <p class="label">Chat emotes</p>
    <EmoteProviders />
  </section>

  <section>
    <span class="eyebrow">Storage</span>
    <p class="label">Downloaded videos</p>
    <p class="muted hint">
      Videos kept here to play and clip: those of VODs from sites other than Kick, Twitch and YouTube, and of those
      three when downloaded with the button at the left of their row{mediaBytes
        ? `. They use ${formatBytes(mediaBytes)}`
        : ""}. Delete one with the crossed-out camera in its row to free up space; its transcript stays.
    </p>
    <div class="segmented" role="radiogroup" aria-label="Video quality">
      {#each HEIGHTS as h (h)}
        <button
          role="radio"
          aria-checked={videoHeight === h}
          class:on={videoHeight === h}
          onclick={() => appSettings.update({ video_height: h })}
        >
          {h}p
        </button>
      {/each}
    </div>
    <p class="muted hint">
      Up to {videoHeight}p, for kept videos and clips. An hour is about {videoSize(videoHeight)}. Applies to videos
      downloaded from now on.
    </p>
    <div class="row-buttons">
      <button onclick={() => api.openVideosDir().catch((e) => (error = String(e)))}>Open folder</button>
      <button class="danger-outline" onclick={deleteAllVideos} disabled={deletingVideos || !mediaBytes}>
        {deletingVideos ? "Deleting…" : "Delete all downloaded videos…"}
      </button>
    </div>
    {#if videosNote}<p class="muted hint">{videosNote}</p>{/if}
    <p class="label">Clips and edits are made as</p>
    <div class="segmented" role="radiogroup" aria-label="Clip format">
      {#each [["mp4", "MP4"], ["mov", "MOV"]] as const as [format, name] (format)}
        <button
          role="radio"
          aria-checked={clipFormat === format}
          class:on={clipFormat === format}
          onclick={() => appSettings.update({ clip_format: format })}
        >
          {name}
        </button>
      {/each}
    </div>
    <p class="muted hint">
      {clipFormat === "mp4"
        ? "H.264 and AAC: small, and read by nearly every video editor."
        : "DNxHR and PCM, for editing: every frame complete, so it scrubs smoothly even on slower computers; also what DaVinci Resolve needs on Linux. About 350 MB a minute at 1080p."}
      Clips already made are made again when next dragged out.
    </p>
    <p class="label">Repair VODscribe</p>
    <p class="muted hint">
      Something not working? This deletes the speech model, tools and unfinished downloads{cacheBytes !== null
        ? ` (${formatBytes(cacheBytes)} in all)`
        : ""}, then sets VODscribe up again. Your library, transcripts, chats and kept videos stay.
    </p>
    <button class="danger-outline" onclick={clearCache} disabled={clearing || step !== null || gpu?.installing}>
      {clearing ? "Repairing…" : "Repair…"}
    </button>
  </section>

  <section>
    <span class="eyebrow">Video downloader</span>
    <p class="muted hint">If VODs from a site stop downloading, updating usually fixes it.</p>
    <button onclick={updateYtDlp} disabled={updating}>{updating ? "Updating…" : "Update video downloader"}</button>
    {#if updating && ytDlpStep}
      <DownloadStep progress={ytDlpStep} bar={false} />
    {:else if ytDlpStatus}
      <p class="muted hint step">{ytDlpStatus}</p>
    {/if}
  </section>

  <section>
    <span class="eyebrow">Updates</span>
    <p class="muted hint">VODscribe checks once a day, and asks before installing anything.</p>
    {#if updates.available}
      <button class="primary" onclick={() => updates.install()} disabled={updates.installing}>
        {updates.installing ? "Updating…" : `Update to ${updates.available.version} and restart`}
      </button>
    {:else}
      <button onclick={() => updates.check(true)} disabled={updates.checking}>
        {updates.checking ? "Checking…" : "Check for updates"}
      </button>
    {/if}
    {#if updates.error}
      <p class="error-text hint">{updates.error}</p>
    {:else if updates.status}
      <p class="muted hint">{updates.status}</p>
    {/if}
  </section>

  <section>
    <span class="eyebrow">Privacy & security</span>
    <p class="hint">
      Your transcripts, audio and searches stay on this computer. There's no account, and VODscribe doesn't collect
      anything about how you use it.
    </p>
    <p class="muted hint">
      VODscribe only goes online to get the VODs you add, to play them, to download the parts it needs to run, and to
      check for updates. Those parts and updates are checked before they're used, to make sure they haven't been
      tampered with. Video files from your computer never leave it.
    </p>
    {#if dataDir}
      <p class="muted hint">Your library is in:</p>
      <code class="path">{dataDir}</code>
    {/if}
    <div class="row-buttons">
      <button onclick={() => api.openDataDir().catch((e) => (error = String(e)))}>Open folder</button>
    </div>
    <div class="row-buttons">
      <button class="link" onclick={() => openUrl(PRIVACY)}
        >More about privacy <Icon name="external" size={12} /></button
      >
      <button class="link" onclick={() => openUrl(LICENSES)}>Licenses <Icon name="external" size={12} /></button>
    </div>
  </section>

  <section>
    <span class="eyebrow">Help</span>
    <p class="muted hint">Something broken? Got an idea?</p>
    <button class="link" onclick={() => openUrl(ISSUES)}
      >Report a problem or suggest something <Icon name="external" size={12} /></button
    >
    <button class="link" onclick={resetSettings}>Reset settings…</button>
  </section>
</div>

<style>
  .settings {
    display: grid;
  }

  section {
    display: grid;
    /* One column as wide as the panel, so a long row wraps instead of widening it. */
    grid-template-columns: minmax(0, 1fr);
    gap: 8px;
    padding: 14px 16px;
    justify-items: start;
  }

  .step {
    font-size: 12px;
  }

  .verified {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 0;
    font-size: 12px;
    color: var(--ok);
  }

  .path {
    font-size: 11px;
    word-break: break-all;
  }

  .row-buttons {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 14px;
  }

  section + section {
    border-top: 1px solid var(--border);
  }

  .options {
    display: grid;
    gap: 6px;
    width: 100%;
  }

  .option {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    cursor: pointer;
    font-size: 12px;
  }

  .option:hover:not(.disabled) {
    background: var(--surface-2);
  }

  .option.checked {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .option.disabled {
    cursor: default;
    opacity: 0.65;
  }

  .option input {
    margin: 2px 0 0;
  }

  .option-text {
    display: grid;
    gap: 1px;
    min-width: 0;
  }

  .option-title {
    font-size: 13px;
    font-weight: 600;
  }

  .detail {
    display: block;
    font-weight: 400;
  }

  .label {
    margin: 0 0 6px;
    font-size: 13px;
  }

  /* A row of choices, the chosen one filled (theme, video quality, clip format). */
  .segmented {
    display: inline-flex;
    margin-bottom: 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .segmented button {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 10px;
    font-size: 12px;
    border: none;
    border-radius: 0;
    background: none;
    color: var(--muted);
  }

  .segmented button + button {
    border-left: 1px solid var(--border);
  }

  .segmented button.on {
    color: var(--text);
    background: var(--accent-soft);
  }

  .hint {
    margin: 0;
    font-size: 12px;
    display: flex;
    gap: 5px;
    align-items: center;
  }

  .progress {
    display: grid;
    gap: 4px;
    font-size: 12px;
    width: 100%;
  }
</style>
