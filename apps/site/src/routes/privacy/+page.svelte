<script lang="ts">
  import { SUPPORT } from "#lib/links.js";
</script>

<svelte:head>
  <title>Privacy · VODscribe</title>
  <link rel="canonical" href="https://vodscribe.com/privacy" />
  <meta property="og:title" content="Privacy · VODscribe" />
  <meta property="og:image" content="https://vodscribe.com/og.jpg" />
  <meta
    name="description"
    content="What VODscribe and vodscribe.com do with your data: nothing leaves your computer."
  />
</svelte:head>

<article class="prose">
  <h1>Privacy</h1>
  <p class="muted">Last updated October 14, 2026</p>

  <p>
    VODscribe works on your computer, not in the cloud. There's no account, and neither the app nor this website
    collects anything about you or how you use them.
  </p>

  <h2>The app</h2>
  <ul>
    <li>
      Your library, transcripts, chat replays, timeline layers and searches are stored only on your computer, in
      VODscribe's data folder (Settings shows where). Video files you add from your computer are read where they are,
      and never copied or uploaded.
    </li>
    <li>VODscribe has no analytics, telemetry or crash reporting.</li>
  </ul>
  <p>It goes online only to:</p>
  <ul>
    <li>
      <strong>Get the VODs you add.</strong> It downloads their details and audio from Kick, Twitch or YouTube, or from the
      site the link is on, the way your browser would. From other sites it downloads the video too, and keeps it on your computer
      to play there, until you delete it or the VOD. When you ask, it downloads a VOD's video, or the part of it you make
      a clip of, from the same place. It only takes links you add to the internet, never to your computer or your local network.
    </li>
    <li>
      <strong>Get their chat.</strong> For Kick and Twitch VODs it reads the chat replay from those sites, as their own pages
      do (web.kick.com and gql.twitch.tv); for YouTube, through the video downloader. Emotes in chat are images from Kick,
      Twitch or YouTube (files.kick.com, static-cdn.jtvnw.net, yt3.ggpht.com), each fetched once and then kept on your computer.
    </li>
    <li>
      <strong>Show the emotes chat types as words, if you turn that on.</strong> Off until you turn on 7TV, BetterTTV or FrankerFaceZ
      in Settings → Appearance. Then, for a Kick or Twitch VOD's chat, it asks the ones you turned on (7tv.io, api.betterttv.net,
      api.frankerfacez.com) which emotes that channel has, by the channel's id (looked up from Twitch, or from kick.com),
      at most once a day per channel. Their images come from those services (cdn.7tv.app, cdn.betterttv.net, cdn.frankerfacez.com)
      and are kept on your computer like the others.
    </li>
    <li>
      <strong>Play them.</strong> YouTube and Twitch VODs play in those sites' own players, which load only when you play
      one. YouTube's player is its privacy-enhanced version (youtube-nocookie.com). Kick VODs stream straight from Kick's
      servers, and Twitch clips from Twitch's, with a link Twitch signs for each viewing (from gql.twitch.tv). Videos from
      other sites play from your computer.
    </li>
    <li>
      <strong>Download what it needs to run.</strong> On first run, and when you ask: the video downloader (yt-dlp, from GitHub),
      the video reader (FFmpeg, from GitHub or ffmpeg.martin-riedl.de), the YouTube helper (Deno, from GitHub) and the speech
      model (from dl.vodscribe.com). The optional GPU add-on comes from GitHub and NVIDIA's packages on PyPI. Every file is
      checked against a known fingerprint before it's used.
    </li>
    <li>
      <strong>Check for updates.</strong> Once a day it fetches a small file from dl.vodscribe.com that names the latest version.
      Updates install only when you say so, and only if they're signed with VODscribe's key.
    </li>
  </ul>
  <p>
    Those sites see what any website sees when you visit: your IP address and what was requested. dl.vodscribe.com is
    hosted by Cloudflare and keeps no logs of its own.
  </p>

  <h2>This website</h2>
  <p>
    vodscribe.com sets no cookies and runs no analytics or ads. It's hosted by Cloudflare, which handles requests to it
    like any web host. The demo runs entirely in your browser; its YouTube and Twitch players load from those sites only
    when you play a VOD. Its chat is a real stream's, with the name of everyone in it replaced by a made-up one.
  </p>

  <h2>Donations</h2>
  <p>
    Support goes through <a href={SUPPORT} target="_blank" rel="noopener">Ko-fi</a>, under its own privacy policy. I see
    what Ko-fi shows creators: the name and message you choose to leave.
  </p>

  <h2>Questions</h2>
  <p>Send a message through <a href={SUPPORT} target="_blank" rel="noopener">Ko-fi</a>.</p>
</article>
