<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, events, type Device, type GpuStatus, type UnlistenFn } from "./api";

  let { oncleared }: { oncleared: () => void } = $props();

  let device = $state<Device>("cpu");
  let cacheBytes = $state<number | null>(null);
  let clearing = $state(false);
  let gpu = $state<GpuStatus | null>(null);
  let step = $state<{ label: string; done: number; total: number } | null>(null);
  let error = $state<string | null>(null);
  const unlisten: UnlistenFn[] = [];
  let timer: ReturnType<typeof setInterval> | undefined;

  const gb = (mb: number) => `${(mb / 1024).toFixed(1)} GB`;

  async function refresh() {
    gpu = await api.gpuStatus();
    cacheBytes = await api.cacheSize();
  }

  const size = (bytes: number) =>
    bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1e6))} MB`;

  async function clearCache() {
    const amount = cacheBytes ? `${size(cacheBytes)} of ` : "";
    const ok = confirm(
      `Delete ${amount}downloaded files? This removes the speech models, the GPU pack, yt-dlp and unfinished downloads. ` +
        "Running jobs are paused. Your library and transcripts are kept, and setup downloads what's needed again.",
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

  onMount(async () => {
    device = (await api.getSettings()).device;
    unlisten.push(
      await events.gpuProgress((label, done, total) => (step = { label, done, total })),
      await events.gpuDone((e) => {
        step = null;
        error = e;
        refresh();
      }),
    );
    await refresh();
    // Free VRAM changes as other programs use the GPU.
    timer = setInterval(refresh, 5000);
  });
  onDestroy(() => {
    clearInterval(timer);
    unlisten.forEach((u) => u());
  });

  async function choose(d: Device) {
    device = d;
    await api.setSettings({ device: d });
  }

  async function install() {
    error = null;
    await api.installGpuPack();
    await refresh();
  }

  async function remove() {
    if (!confirm("Remove the GPU pack? Transcription will use the CPU.")) return;
    if (device === "gpu") await choose("cpu");
    await api.removeGpuPack().catch((e) => (error = String(e)));
    await refresh();
  }

  // Why GPU can't be chosen right now, if it can't.
  let blocker = $derived.by(() => {
    if (!gpu) return "Checking…";
    if (!gpu.supported) return "Not available on this operating system.";
    if (!gpu.worker) return "This build doesn't include GPU support.";
    if (!gpu.gpu) return "No NVIDIA GPU found.";
    if (!gpu.installed) return "Needs the GPU pack, a one-time download of about 3 GB.";
    return null;
  });
  let busy = $derived(gpu?.gpu && gpu.installed && gpu.gpu.free_mb < gpu.min_free_mb);
</script>

<div class="settings">
  <fieldset>
    <legend>Transcribe with</legend>
    <label>
      <input type="radio" name="device" checked={device === "cpu"} onchange={() => choose("cpu")} />
      <span>CPU</span>
    </label>
    <label class:disabled={blocker !== null}>
      <input
        type="radio"
        name="device"
        checked={device === "gpu"}
        disabled={blocker !== null}
        onchange={() => choose("gpu")}
      />
      <span>
        GPU
        {#if gpu?.gpu}<span class="muted">· {gpu.gpu.name}, {gb(gpu.gpu.free_mb)} free</span>{/if}
      </span>
    </label>
    {#if blocker}
      <p class="muted hint">{blocker}</p>
    {:else if busy && gpu}
      <p class="hint warn">
        The GPU is busy. VODs will use the CPU until {gb(gpu.min_free_mb)} is free.
      </p>
    {/if}
  </fieldset>

  {#if gpu?.supported && gpu.worker && gpu.gpu}
    {#if step || gpu.installing}
      <div class="progress">
        <span class="muted">Downloading {step?.label ?? "GPU pack"}…</span>
        <div class="bar"><div style:width="{step && step.total ? (step.done / step.total) * 100 : 0}%"></div></div>
      </div>
    {:else if !gpu.installed}
      <button onclick={install}>Download GPU pack</button>
    {:else}
      <button class="link" onclick={remove}>Remove GPU pack</button>
    {/if}
  {/if}
  {#if error}<p class="error-text hint">{error}</p>{/if}
  <p class="muted hint">Changes apply from the next VOD.</p>

  <div class="storage">
    <span class="label">Storage</span>
    <p class="muted hint">
      Downloaded files{cacheBytes !== null ? `: ${size(cacheBytes)}` : ""}. Your library isn't included.
    </p>
    <button class="danger" onclick={clearCache} disabled={clearing || step !== null || gpu?.installing}>
      {clearing ? "Clearing…" : "Clear downloads…"}
    </button>
  </div>
</div>

<style>
  .settings {
    padding: 14px;
    display: grid;
    gap: 8px;
  }

  fieldset {
    border: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
  }

  legend {
    font-weight: 600;
    font-size: 12px;
    margin-bottom: 4px;
  }

  label {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }

  label.disabled {
    opacity: 0.6;
  }

  .hint {
    margin: 0;
    font-size: 12px;
  }

  .warn {
    color: var(--danger);
  }

  .progress {
    display: grid;
    gap: 4px;
    font-size: 12px;
  }

  .bar {
    height: 4px;
    border-radius: 2px;
    background: var(--border);
    overflow: hidden;
  }

  .bar > div {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s;
  }

  button {
    justify-self: start;
  }

  .storage {
    display: grid;
    gap: 6px;
    margin-top: 6px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }

  .label {
    font-weight: 600;
    font-size: 12px;
  }

  .danger {
    color: var(--danger);
    border-color: var(--danger);
  }

  .danger:hover:not(:disabled) {
    background: var(--danger);
    color: var(--surface);
  }
</style>
