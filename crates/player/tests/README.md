# vodscribe-player tests

Each test makes its videos with ffmpeg and reads back what the compositor drew. They skip without ffmpeg on `PATH`, unless `VODSCRIBE_TESTS_NEED_ALL` is set, as on CI.

- `frames.rs`: seeks and steps land on exactly the frame asked for.
- `colours.rs`: every common kind of YUV and RGB comes out the right colour.
- `media.rs`: awkward files, such as long GOPs, variable frame rates, rotation and odd sizes.
- `preview.rs`: the preview decoder shows a frame without seeking, then plays on.
- `growing.rs`: footage still downloading plays as far as it's in, and on.
- `waiting.rs`: footage not here yet waits for a site's VOD and plays black in an edit.
- `shown_later.rs`: a site's first segment, whose first frame is shown later than it's decoded.
- `sound_apart.rs`: a picture and its sound from two files, played as one.
- `sound_loss.rs`: the sound output going away while playing; run by hand with `VODSCRIBE_SOUND_OFF` and `VODSCRIBE_SOUND_ON`.
- `empty.rs`: an edit with no pieces plays nothing and doesn't spin.
- `diagnosis.rs`: what the player reports of itself for a stall.
