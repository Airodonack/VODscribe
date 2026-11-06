# Test fixtures

Recorded files the scenarios use, and the scripts that made them. Videos are made at test time by `lib/fixtures.mjs`, so none are kept here.

- `eagle-has-landed.mp3`: Apollo 11's landing, 5 seconds, "Houston, Tranquility Base here. The Eagle has landed."; NASA, public domain, from [Wikimedia Commons](https://commons.wikimedia.org/wiki/File:Apollo_11_Eagle_Has_Landed.mp3).
- `library-0.3.0.sqlite`, `settings-0.3.0.json`: the landing transcribed and the dark theme, as 0.3.0 left them, for `upgrade`.
- `library-0.5.0.sqlite`, `settings-0.5.0.json`: the same as 0.5.0 left them, with ten minutes of the landing paused part-way, for `upgrade-0.5.0`.
- `library-0.4.0.sqlite`, `library-0.5.1.sqlite`, `library-0.5.2-dev.sqlite` and their `settings-<version>.json`: the upgrade matrix, every kind of row each release kept.
- `annotations-minute.json`, `annotations-minute.chat.json`: a made-up VODscription and chat for a minute-long VOD, for `annotations`.
- `streamer-cards.json`: made-up streamer cards and their index, for `streamer-card`.
- `make-old-library.mjs`: makes an old release's library by driving that release.
- `make-matrix-library.mjs`: makes a matrix library from that release's own migrations.
- `upgrade-matrix.mjs`: the checks every `upgrade-<version>` scenario shares.
- `large-library.mjs`: grows a library to 60 VODs of two hours' talk, for `search-large` and `perf-library`.

The libraries keep the files' folder as a placeholder, `{FIXTURES}` or `{SPEECH}`, and the annotations their platform and id as `{PLATFORM}` and `{ID}`; the scenarios fill them in. When a release changes the library's format or the settings, add it to `RELEASES` in `make-matrix-library.mjs`, run `make-matrix-library.mjs <version>`, and add its scenario file.
