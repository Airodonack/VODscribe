# The library commands' tests

- `tests.rs`: the files in `tests/` that hold the specs of `vods.rs`, and what they share: a transcription asked for and seen through the commands, and the lock-order check.
- `tests/adding.rs`: adding VODs, and refreshing their details.
- `tests/list.rs`: listing and ordering VODs.
- `tests/removing.rs`: removing VODs, which ends their transcription and chat download, erasing them, and deleting their videos.
- `tests/transcribing.rs`: asking for a VOD's transcription.
- `tests/reading.rs`: reading a VOD's transcript and captions, the names `from:` suggests, showing and opening its file, and playing a Twitch clip or a Kick VOD.
