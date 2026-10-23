# Test fixtures

- `eagle-has-landed.mp3`: Apollo 11's landing ("Houston, Tranquility Base here. The Eagle has landed."), 5 seconds. NASA, public domain. From [Wikimedia Commons](https://commons.wikimedia.org/wiki/File:Apollo_11_Eagle_Has_Landed.mp3), originally [nasa.gov](https://www.nasa.gov/connect/sounds/index.html).
- `library-0.3.0.sqlite`, `settings-0.3.0.json`: a library and settings as VODscribe 0.3.0 left them (the landing above, transcribed; the dark theme), for the upgrade scenario. Made by `make-old-library.mjs` driving the 0.3.0 release; the file's folder is the placeholder `{FIXTURES}`.
- `library-0.5.0.sqlite`, `settings-0.5.0.json`: the same as 0.5.0 left them, with ten minutes of the landing looped (`longSpeech`) paused part-way through transcribing, for the upgrade-0.5.0 scenario. Made by `make-old-library.mjs` driving CI's 0.5.0 Linux build; the looped file's folder is the placeholder `{SPEECH}`.
