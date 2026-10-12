<svelte:head>
  <title>Licenses · VODscribe</title>
  <meta name="description" content="The open-source software and models VODscribe is built on, and their licenses." />
</svelte:head>

<article class="prose">
  <h1>Licenses</h1>
  <p>VODscribe is built on, and downloads, the work of these projects. Thank you to everyone behind them.</p>

  <h2>Downloaded by the app</h2>
  <ul>
    <li>
      <strong>Parakeet TDT 0.6B v3</strong> speech model by NVIDIA, licensed
      <a href="https://creativecommons.org/licenses/by/4.0/" rel="noopener">CC BY 4.0</a>. VODscribe distributes it
      converted to ONNX and quantized (int8 for processors, fp16 for GPUs). Original:
      <a href="https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3" rel="noopener"
        >huggingface.co/nvidia/parakeet-tdt-0.6b-v3</a
      >.
    </li>
    <li>
      <strong>yt-dlp</strong>, <a href="https://github.com/yt-dlp/yt-dlp" rel="noopener">github.com/yt-dlp/yt-dlp</a>,
      released into the public domain (Unlicense).
    </li>
    <li>
      <strong>FFmpeg</strong>, <a href="https://ffmpeg.org" rel="noopener">ffmpeg.org</a>, licensed GPL v3. The app
      downloads builds from <a href="https://github.com/BtbN/FFmpeg-Builds" rel="noopener">BtbN/FFmpeg-Builds</a>
      (Windows, Linux) and <a href="https://ffmpeg.martin-riedl.de" rel="noopener">ffmpeg.martin-riedl.de</a> (macOS), which
      publish their source.
    </li>
    <li>
      <strong>Deno</strong>, <a href="https://github.com/denoland/deno" rel="noopener">github.com/denoland/deno</a>, MIT
      license.
    </li>
    <li>
      <strong>GPU add-on:</strong> ONNX Runtime (MIT) as packaged by sherpa-onnx, and NVIDIA's CUDA, cuBLAS, cuRAND and cuDNN
      libraries from PyPI, under the NVIDIA software license.
    </li>
  </ul>

  <h2>Built into the app</h2>
  <ul>
    <li>
      <strong>sherpa-onnx</strong> (Apache 2.0) and <strong>ONNX Runtime</strong> (MIT), which run the speech model.
    </li>
    <li>
      <strong>Tauri</strong> (MIT or Apache 2.0) and <strong>Svelte</strong> (MIT), the app's window and interface.
    </li>
    <li><strong>SQLite</strong> (public domain), which stores and searches transcripts.</li>
    <li><strong>hls.js</strong> (Apache 2.0), which plays Kick VODs.</li>
    <li>Icons from <strong>Lucide</strong> (ISC).</li>
    <li>And many Rust crates and npm packages, each under its own open-source license.</li>
  </ul>

  <h2>Demo VODs</h2>
  <ul>
    <li>
      “NASA's Artemis II News Conference with Moon Astronauts” by NASA, a work of the US government in the public
      domain.
    </li>
    <li>“Standard Output” by TwitchDev, shown through Twitch's own player.</li>
  </ul>
</article>
