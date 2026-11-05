<script lang="ts">
  import { HELP_MAILTO, ISSUES, SECURITY_REPORT, SUPPORT_EMAIL } from "#lib/links.js";
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

  <h2>Email</h2>
  <p>
    Write to <a href={HELP_MAILTO}>{SUPPORT_EMAIL}</a> about a problem, a question about your data, or to have it removed.
  </p>

  <h2>Ideas and bug reports</h2>
  <p>
    Post them on the <a href={ISSUES} target="_blank" rel="noopener">issue tracker</a>, where anyone can read and add to
    them.
  </p>

  <h2>Security problems</h2>
  <p>
    Report them privately through <a href={SECURITY_REPORT} target="_blank" rel="noopener">GitHub</a>, not by email or
    in a public issue.
  </p>

  <h2>What helps a report</h2>
  <p>The VODscribe version, shown beside its name at the top of the library, and your system, such as Windows 11.</p>
</article>
