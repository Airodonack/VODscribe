# vodscribe-player sources

- `lib.rs`: the player, tying the parts below together.
- `decode.rs`: frames and sound read by ffmpeg, one process per stream.
- `timeline.rs`: what plays when, for a file or an edit's pieces end to end.
- `clock.rs`: the player's time, which follows the sound, and the frame showing.
- `sound.rs`: sound out through the default output, with cpal.
- `compose.rs`: draws the picture and what goes over it with wgpu.
- `present.rs`: the native surface over the page's player box.
- `present_linux.rs`: that surface on Wayland and X11.
- `present_macos.rs`: that surface as a layer in the window's view.
- `present_windows.rs`: that surface as a child window, with GDI where Direct3D is software only.
- `storyboard.rs`: small keyframe pictures for the timeline's thumbnail.
- [`bin`](bin): a bare window to play a file in.
