<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "desktop/lib/Icon.svelte";
  import Demo from "#lib/Demo.svelte";
  import DownloadNote from "#lib/DownloadNote.svelte";
  import { DOWNLOADS, detectMacChip, detectOs, primaryDownload, type MacChip, type Os } from "#lib/downloads.js";
  import { ISSUES, SOURCE, SUPPORT, UNSIGNED } from "#lib/links.js";

  // The main button offers this visitor's OS; every installer is listed below it.
  let os = $state<Os | null>(null);
  let macChip = $state<MacChip | null>(null);
  onMount(() => {
    os = detectOs();
    if (os === "mac") detectMacChip().then((chip) => (macChip = chip));
  });
  let primary = $derived(os ? primaryDownload(os, macChip) : undefined);
  // Which download's heads-up is open (DownloadNote): the download itself goes ahead.
  let note = $state<Os | null>(null);

  const FAQ = [
    {
      q: "Is it free?",
      a: "Yes. No account, no ads, no trial. If it saves you time, you can chip in on Ko-fi to help cover hosting and code signing.",
    },
    {
      q: "Do my VODs or transcripts get uploaded anywhere?",
      a: "No. VODscribe downloads a VOD's audio to your computer and transcribes it there. Transcripts and searches never leave your computer, and VODscribe collects nothing about how you use it.",
    },
    {
      q: "Which VODs can it transcribe?",
      a: "Kick, Twitch and YouTube links: past broadcasts, highlights, clips and uploads. Links from over a thousand other sites work too, from Rumble and Dailymotion to SoundCloud, and their video is kept on your computer to play. It also takes video and audio files from your computer.",
    },
    {
      q: "Does it show the chat?",
      a: "Yes, for Kick, Twitch and YouTube livestreams: the chat replay sits beside the transcript, lined up with what was being said, and search covers it too. A timeline shows how busy chat was over the whole stream, and can mark the laughs, hype, subs and tips, or any words and emotes you choose.",
    },
    {
      q: "Which languages does it understand?",
      a: "English and 24 other European languages, using NVIDIA's Parakeet speech model. It's very good with clear speech; names and game terms sometimes come out spelled differently.",
    },
    {
      q: "How fast is it?",
      a: "It depends on your computer. A recent processor transcribes an hour of VOD in minutes. On Windows and Linux, an NVIDIA graphics card with the free GPU add-on is several times faster.",
    },
    {
      q: "Why does Windows or macOS warn me before opening it?",
      a: `VODscribe ${UNSIGNED} On Windows, choose More info, then Run anyway. On a Mac, drag VODscribe into Applications and open it once, choose Done (not Move to Trash), then go to System Settings › Privacy & Security, choose Open Anyway, choose Open Anyway again when macOS asks, and enter your password (on macOS 14 or earlier, Control-click it in Applications and choose Open).`,
    },
    {
      q: "Something's broken, or I have an idea. Where do I say so?",
      a: "On the issue tracker, which the person who makes VODscribe reads.",
      link: { text: "Open the issue tracker", href: ISSUES },
    },
    {
      q: "Can I see the code?",
      a: "Yes. VODscribe is source-available under the Functional Source License: read it, build it, change it for yourself; just don't sell a competing product from it. Each version becomes Apache 2.0 two years after release.",
      link: { text: "See the code on GitHub", href: SOURCE },
    },
  ];
</script>

<svelte:head>
  <title>VODscribe: search everything said in your VODs</title>
  <meta
    name="description"
    content="VODscribe transcribes Kick, Twitch and YouTube VODs, and videos from other sites, on your computer, so you can find any moment by what was said. Free for Windows, macOS and Linux."
  />
  <meta property="og:title" content="VODscribe: search everything said in your VODs" />
  <meta
    property="og:description"
    content="Transcribe Kick, Twitch and YouTube VODs on your computer and find any moment by what was said."
  />
  <meta property="og:type" content="website" />
  <meta property="og:url" content="https://vodscribe.com/" />
  <meta property="og:image" content="https://vodscribe.com/og.jpg" />
  <link rel="canonical" href="https://vodscribe.com/" />
  <meta name="twitter:card" content="summary_large_image" />
</svelte:head>

<main>
  <section class="hero">
    <h1>Search everything said in your VODs</h1>
    <p class="lead">
      VODscribe transcribes Kick, Twitch and YouTube VODs on your computer. Find any moment by what was said, and jump
      straight to it.
    </p>
    <div class="ctas">
      <a class="button primary big" href={primary?.url ?? "#download"} onclick={() => (note = primary?.os ?? null)}>
        <Icon name="download" size={18} />
        {primary ? `Download for ${primary.label}` : os === "mac" ? "Download for macOS" : "Download"}
      </a>
      <a class="button big" href={SUPPORT} target="_blank" rel="noopener">
        <span class="heart">❤︎</span> Support on Ko-fi
      </a>
    </div>
    <p class="small muted">Free for Windows, macOS and Linux</p>
  </section>

  <section class="demo">
    <p class="demo-intro">
      <strong>Try it here with the transcribed VODs below.</strong>
      <span class="muted">Search them, or pick one to play. Transcribing your own VODs needs the app.</span>
    </p>
    <div class="wide">
      <Demo />
    </div>
    <div class="narrow">
      <img
        src="/screenshot.jpg"
        alt="VODscribe with a NASA VOD open and every mention of “heat shield” found"
        width="1132"
        height="623"
      />
      <p class="muted small">The live demo needs a bigger screen. Open this page on a computer to try it.</p>
    </div>
  </section>

  <section class="features">
    <div>
      <Icon name="search" size={22} />
      <h2>Every word, searchable</h2>
      <p class="muted">
        Type a phrase to see every time it was said or written in chat, across all your VODs. Click one to play from
        there.
      </p>
    </div>
    <div>
      <Icon name="message" size={22} />
      <h2>Chat beside the transcript</h2>
      <p class="muted">
        The stream's chat, lined up with what was said, and a timeline of where it went wild: laughs, hype, subs and
        tips.
      </p>
    </div>
    <div>
      <Icon name="film" size={22} />
      <h2>Kick, Twitch, YouTube and more</h2>
      <p class="muted">Paste a link or drop a video file. VODs queue up and transcribe one after another.</p>
    </div>
    <div>
      <Icon name="shield" size={22} />
      <h2>Runs on your computer</h2>
      <p class="muted">
        No account, no uploads, nothing collected. Any recent computer will do; an NVIDIA graphics card makes it several
        times faster.
      </p>
    </div>
  </section>

  <section class="download" id="download">
    <h2>Download VODscribe</h2>
    <ul class="files">
      {#each DOWNLOADS as d (d.url)}
        <li class:mine={d.os === os && (!macChip || d.url.includes(`macos-${macChip}`))}>
          <a href={d.url} onclick={() => (note = d.os)}>
            <Icon name="download" size={16} />
            <span class="name">{d.label}</span>
            <span class="muted">{d.detail}</span>
          </a>
        </li>
      {/each}
    </ul>
    {#if os === "mac" && !macChip}
      <p class="small which">
        Not sure which Mac you have? In the Apple menu, choose About This Mac: a “Chip” line (M1, M2 and later) means
        Apple Silicon; a “Processor” line means Intel.
      </p>
    {/if}
    <p class="muted small">
      For 64-bit Windows 10 and 11, macOS 12 and later (Apple Silicon and Intel) and Linux. On first run it downloads
      what it needs to transcribe (about 1 GB), checking each file before using it.
    </p>
  </section>

  <section class="faq">
    <h2>Questions</h2>
    {#each FAQ as item (item.q)}
      <details>
        <summary>{item.q}</summary>
        <p class="muted">
          {item.a}
          {#if item.link}<a href={item.link.href} rel="noopener">{item.link.text}</a>{/if}
        </p>
      </details>
    {/each}
  </section>
</main>

<DownloadNote bind:os={note} />

<style>
  main {
    max-width: 1180px;
    margin: 0 auto;
    padding: 0 24px;
  }

  section {
    margin: 0 auto 88px;
  }

  .hero {
    max-width: 760px;
    padding-top: 48px;
    text-align: center;
  }

  h1 {
    margin: 0 0 16px;
    font-size: clamp(34px, 6vw, 56px);
    line-height: 1.08;
    letter-spacing: -0.03em;
  }

  .lead {
    margin: 0 auto 28px;
    max-width: 600px;
    font-size: 19px;
    color: var(--muted);
  }

  .ctas {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 12px;
    margin-bottom: 14px;
  }

  .button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    text-decoration: none;
    font-weight: 600;
  }

  .button:hover {
    background: var(--surface-2);
  }

  .button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }

  .button.primary:hover {
    filter: brightness(1.08);
  }

  .big {
    padding: 12px 22px;
    font-size: 16px;
  }

  .heart {
    color: #e5484d;
  }

  .small {
    font-size: 13px;
  }

  .demo {
    text-align: center;
  }

  .demo-intro {
    margin: 0 0 14px;
  }

  .narrow {
    display: none;
  }

  .narrow img {
    width: 100%;
    height: auto;
    border: 1px solid var(--border);
    border-radius: 12px;
  }

  @media (max-width: 820px) {
    .wide {
      display: none;
    }

    .narrow {
      display: block;
    }
  }

  .features {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 28px;
  }

  .features :global(svg) {
    color: var(--accent);
  }

  .features h2 {
    margin: 10px 0 6px;
    font-size: 16px;
  }

  .features p {
    margin: 0;
  }

  .download {
    max-width: 640px;
    text-align: center;
  }

  h2 {
    font-size: 28px;
    letter-spacing: -0.02em;
    margin: 0 0 20px;
  }

  .files {
    list-style: none;
    margin: 0 0 16px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    text-align: left;
  }

  .files li + li {
    border-top: 1px solid var(--border);
  }

  .files a {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    color: var(--text);
    text-decoration: none;
  }

  .files a:hover {
    background: var(--surface-2);
  }

  .files .name {
    font-weight: 600;
  }

  .files .muted {
    margin-left: auto;
    font-size: 13px;
    text-align: right;
  }

  .which {
    margin: 0 0 12px;
  }

  .files li.mine .name {
    color: var(--accent);
  }

  .faq {
    max-width: 720px;
  }

  details {
    border-bottom: 1px solid var(--border);
    padding: 14px 0;
  }

  summary {
    cursor: pointer;
    font-weight: 600;
  }

  details p {
    margin: 8px 0 0;
  }
</style>
