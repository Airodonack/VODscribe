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
      a: "<p>They need a video decoder that some systems leave out, and VODscribe's package brings it in. If it was skipped: on Ubuntu or Debian, install <code>gstreamer1.0-libav</code>. On Fedora, VODscribe needs <code>openh264</code> from Cisco's repository; if the install says nothing provides it, turn that repository on with <code>sudo dnf config-manager setopt fedora-cisco-openh264.enabled=1</code> and install again.</p>",
    },
    {
      q: "DaVinci Resolve on Linux won't take my clips",
      a: "<p>Resolve on Linux can't open MP4, the format VODscribe makes clips in by default. Drop one on Resolve and VODscribe offers to switch to MOV; or pick MOV DNxHR in Settings › Encoding. Then drag the clip again.</p>",
    },
    {
      q: "How do I search?",
      a: '<p>Type what was said. The search box also takes:</p><div class="table"><table><tbody><tr><td><code>big win</code></td><td>those words in order, close together</td></tr><tr><td><code>&quot;big win&quot;</code></td><td>exactly that phrase, whole words only</td></tr><tr><td><code>-word</code> or <code>-&quot;a phrase&quot;</code></td><td>leaves out matches with it nearby</td></tr><tr><td><code>a OR b</code></td><td>either one</td></tr><tr><td><code>a AND b</code></td><td>both; AND binds tighter than OR</td></tr><tr><td><code>(a OR b) c</code></td><td>brackets group</td></tr><tr><td><code>from:name</code></td><td>said or written by them; from:a,b for either</td></tr><tr><td><code>in:chat</code> or <code>in:audio</code></td><td>only chat, or only what&#39;s said</td></tr><tr><td><code>within:20s</code></td><td>how close a match&#39;s words must be: 2m, 1h; at most an hour</td></tr><tr><td><code>after:1:20:00</code> or <code>before:2h</code></td><td>only that part of the VOD</td></tr></tbody></table></div><p>Filters apply to the whole search: <code>from:x "a" OR "b"</code> is a or b, both from x. Who, where and how close can also be picked beside the search box.</p>',
    },
    {
      q: "I need accessibility features",
      a: `<p>Tell us what would help. Whatever you use (a screen reader, the keyboard alone, larger text, more contrast or less motion), we'd like to know where VODscribe gets in your way. <a href="${IDEA_FORM}" target="_blank" rel="noopener">Suggest an idea</a> on the issue tracker or <a href="${HELP_MAILTO}">write to support</a>, and say what you use.</p><p>What's there now: every key can be changed in Settings › Controls, the theme can be light or dark, and a transcribed VOD has captions and a transcript you can search.</p>`,
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
      <div class="muted answer">{@html item.a}</div>
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

  .answer :global(p) {
    margin: 8px 0 0;
  }

  .answer :global(.table) {
    margin-top: 8px;
    overflow-x: auto;
  }

  .answer :global(table) {
    border-collapse: collapse;
  }

  .answer :global(td) {
    padding: 4px 12px 4px 0;
    vertical-align: top;
  }

  .answer :global(td:first-child) {
    white-space: nowrap;
  }
</style>
