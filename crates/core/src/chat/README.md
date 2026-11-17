# Chat replays' tests

- `tests.rs`: the files in `tests/` that hold the specs of `chat.rs`, and the helpers they share.
- `tests/parse.rs`: what each site's chat says, read: times, emotes, subs, gifts and tips, and when the VOD started.
- `tests/fetch.rs`: fetching a chat into the library: slices at once, kept as it arrives, carrying on where it stopped, pages asked again, Twitch's end and clips, YouTube's as yt-dlp writes it.
