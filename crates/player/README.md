# vodscribe-player

The app's own video player, for files, kept and downloaded videos and edits. It decodes with the ffmpeg the app ships, keeps the picture to the sound, and draws with wgpu on a native surface over the page.

- `Cargo.toml`: the crate, its `vodscribe-play` binary, and the `counts` feature the tests use.
- [`src`](src): the code.
- [`tests`](tests): the player on generated videos, checked by their pixels.
