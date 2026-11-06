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

  <h2>Twitch or YouTube don't play on Linux</h2>
  <p>
    They need a video decoder that some systems leave out, and VODscribe's package brings it in. If it was skipped: on
    Ubuntu or Debian, install <code>gstreamer1.0-libav</code>. On Fedora, VODscribe needs <code>openh264</code> from
    Cisco's repository; if the install says nothing provides it, turn that repository on with
    <code>sudo dnf config-manager setopt fedora-cisco-openh264.enabled=1</code> and install again.
  </p>

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
