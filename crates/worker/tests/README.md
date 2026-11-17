# vodscribe-worker tests

- `protocol.rs`: the worker process as the app runs it, and the kit's fake worker beside it. The tests that need the speech model download the CPU one into this target folder's `tmp`, once, and skip where there is no network or no ffmpeg.
