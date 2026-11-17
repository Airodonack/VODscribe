# vodscribe-player tests

Each test makes its videos with ffmpeg and reads back what was decoded or drawn, but `fake_decode.rs`, which checks the test kit's fake ffmpeg against the real one, and the tests that must force an order (in `playing.rs` and `preview_close.rs`), which hold the fake's decodes at a gate. Without ffmpeg on `PATH` they skip what needs it, unless `VODSCRIBE_TESTS_NEED_ALL` is set, as on CI.

- `playing.rs`: where a seek lands, that it leaves playing or paused as it was, Play at the end, the speed, a timeline changed under it.
- `resume.rs`: Play after a pause goes on with the decoder it had.
- `decoding.rs`: video at 23.976, 29.97 and 59.94 fps frame by frame, and the sound at each speed.
- `timeline.rs`: what plays when, for a VOD as it downloads and an edit's pieces.
- `frames.rs`: seeks and steps land on exactly the frame asked for.
- `colours.rs`: every common kind of YUV and RGB comes out the right colour.
- `media.rs`: awkward files, such as long GOPs, variable frame rates, rotation and odd sizes.
- `preview.rs`: the preview decoder shows a frame without seeking, then plays on.
- `preview_close.rs`: closing the player doesn't wait for a preview being decoded.
- `growing.rs`: footage still downloading plays as far as it's in, and on.
- `waiting.rs`: footage not here yet waits for a site's VOD and plays black in an edit.
- `shown_later.rs`: a site's first segment, whose first frame is shown later than it's decoded.
- `sound_apart.rs`: a picture and its sound from two files, played as one.
- `sound_idle.rs`: the sound output stream stops while paused and starts again on Play.
- `sound_loss.rs`: the sound output going away while playing; run by hand with `VODSCRIBE_SOUND_OFF` and `VODSCRIBE_SOUND_ON`.
- `empty.rs`: an edit with no pieces plays nothing and doesn't spin.
- `fake_decode.rs`: the test kit's fake ffmpeg decodes frames, sound and storyboards as the real one does.
- `diagnosis.rs`: what the player reports of itself for a stall.
- [`common`](common): what the tests share.
