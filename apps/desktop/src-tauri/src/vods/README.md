# The library commands' tests

- `tests.rs`: the specs of `vods.rs` not yet in a file of their own, the files in `tests/` that hold the rest, and the lock-order check they share.
- `tests/adding.rs`: adding VODs, and refreshing their details.
- `tests/list.rs`: listing and ordering VODs.
- `tests/removing.rs`: removing VODs, which ends their transcription, erasing them, and deleting their videos.
- `tests/transcribing.rs`: asking for a VOD's transcription.
- `tests/reading.rs`: reading a VOD's transcript and captions, the names `from:` suggests, and showing and opening its file.
