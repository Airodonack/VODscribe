# The shell's code

The commands the page calls, the queues that do the work, and the native parts of the window.

- `main.rs`: starts the app.
- `lib.rs`: the app's state and startup, and the list of commands.
- `vods.rs`: the library's commands: adding, ordering and controlling VODs, refreshing a site's VOD's details, and reading transcripts.
- `setup.rs`: first-run setup, settings, the GPU add-on and storage.
- `recovery.rs`: the recovery screen's commands, when the library won't open.
- `worker.rs`: the transcription queue, downloading in one stage and transcribing in another.
- `chats.rs`: chat replays, fetched apart from that queue.
- `jobs.rs`: clips, renders, video downloads and storyboards, one at a time.
- `job_list.rs`: the jobs list the library shows.
- `clips.rs`: clips' commands, and the queue that cuts them.
- `resolve.rs`: MP4 clips and edits dropped on DaVinci Resolve on Linux, which can't open them, are held back, and the page offers MOV.
- `edits.rs`: edits' commands, and their rendering.
- `footage.rs`: where a stretch plays from, and the downloads of sections edits need.
- `videos.rs`: downloading a VOD's whole video on request.
- `storyboards.rs`: keyframe pictures of the footage on this computer.
- `thumbnails.rs`: VOD thumbnails for the page.
- `annotations.rs`: importing VODscriptions, asking whether a VOD has one, and fetching streamer cards.
- `history.rs`: undo and redo, per document.
- `clipboard.rs`: Ctrl+C and Ctrl+V.
- `choices.rs`: the page's small choices, read at start and written as they change.
- `files.rs`: deleting made files, later where Windows still has them open.
- `player.rs`: the app's own player, driven by the page.
- `screen.rs`: the native surface the player draws on.
- `embed.rs`, `embed_shim.js`: a site's video drawn in the app's player under the site's; the `embed-takeover` feature only.
- `embed_shim.test.mjs`: the shim's tests, in a stand-in of YouTube's embed frame.
- `app_server.rs`: serves the page from `http://localhost` in release builds, since YouTube's and Twitch's players need an http page.
- `media.rs`: serves local files to the page's `<video>`.
- `loopback.rs`: what those local servers share; they answer on loopback only.
- `page.rs`: sends the page its events, from any thread.
- `webview.rs`: the window's WebKitGTK view on Linux.
- `receive.rs`: files dropped with no path, written into the library.
- `e2e.rs`: the end-to-end tests' way in; the `e2e` feature only.
- [`drag_source`](drag_source): dragging clips out before they're made.
- [`drop_target`](drop_target): taking drops on Windows, macOS and Linux, virtual files and file promises included.
