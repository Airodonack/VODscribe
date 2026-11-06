<script lang="ts">
  import {
    BUG_FORM,
    HELP_MAILTO,
    IDEA_FORM,
    ISSUES,
    REPORT_MAILTO,
    SECURITY_REPORT,
    SUPPORT_EMAIL,
  } from "#lib/links.js";

  /** Answers for problems people hit; plain text with code in <code>. */
  const FAQ = [
    {
      q: "Twitch or YouTube don't play on Linux",
      a: "They need a video decoder that some systems leave out, and VODscribe's package brings it in. If it was skipped: on Ubuntu or Debian, install <code>gstreamer1.0-libav</code>. On Fedora, VODscribe needs <code>openh264</code> from Cisco's repository; if the install says nothing provides it, turn that repository on with <code>sudo dnf config-manager setopt fedora-cisco-openh264.enabled=1</code> and install again.",
    },
    {
      q: "DaVinci Resolve on Linux won't take my clips",
      a: "Resolve on Linux can't open MP4, the format VODscribe makes clips in by default. Drop one on Resolve and VODscribe offers to switch to MOV; or pick MOV DNxHR in Settings › Encoding. Then drag the clip again.",
    },
  ];
</script>

<svelte:head>
  <title>Support · VODscribe</title>
  <link rel="canonical" href="https://vodscribe.com/support" />
  <meta property="og:title" content="Support · VODscribe" />
  <meta property="og:image" content="https://vodscribe.com/og.jpg" />
  <meta name="description" content="How to get help with VODscribe, report a problem or suggest an idea." />
</svelte:head>

<article class="prose">
  <h1>Support</h1>

  <h2>Questions</h2>
  {#each FAQ as item (item.q)}
    <details>
      <summary>{item.q}</summary>
      <p class="muted">{@html item.a}</p>
    </details>
  {/each}

  <h2>Bugs and ideas</h2>
  <p>
    <a href={BUG_FORM} target="_blank" rel="noopener">Report a bug</a> or
    <a href={IDEA_FORM} target="_blank" rel="noopener">suggest an idea</a> on the
    <a href={ISSUES} target="_blank" rel="noopener">issue tracker</a>, where anyone can read and add to them.
  </p>

  <h2>Email</h2>
  <p>
    <a href={REPORT_MAILTO}>Write to support</a> about a problem. For a question about your data, or to have it removed,
    write to <a href={HELP_MAILTO}>{SUPPORT_EMAIL}</a>.
  </p>

  <h2>Security problems</h2>
  <p>
    Report them privately through <a href={SECURITY_REPORT} target="_blank" rel="noopener">GitHub</a>, not by email or
    in a public issue.
  </p>
</article>

<style>
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
