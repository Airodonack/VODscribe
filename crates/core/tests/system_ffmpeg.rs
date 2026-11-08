//! The ffmpeg found on PATH (the system's, with none of the app's own in
//! its bin) is the one a format is checked against, and a format it can't
//! make is said to be the system's, not VODscribe's. Its own binary, since
//! it changes PATH.

#[cfg(unix)]
#[test]
fn a_format_the_system_s_ffmpeg_cant_make_is_said_to_be_the_system_s() {
    use std::os::unix::fs::PermissionsExt;
    use vodscribe_core::settings::{ClipCodec, ClipEncoding, ClipQuality};
    let dir = std::env::temp_dir().join(format!("vodscribe-system-ffmpeg-{}", std::process::id()));
    let (bin, system) = (dir.join("bin"), dir.join("usr-bin"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&system).unwrap();
    // A distribution's ffmpeg without libx265 (Fedora's ffmpeg-free, say).
    let ffmpeg = system.join("ffmpeg");
    std::fs::write(
        &ffmpeg,
        // (printf, a builtin: nothing else is on this PATH.)
        "#!/bin/sh\nprintf 'Encoders:\\n V..... = Video\\n ------\\n V....D libx264 H.264\\n A....D aac AAC\\n V....D dnxhd DNxHD\\n V....D prores_ks ProRes\\n A....D pcm_s16le PCM\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&ffmpeg, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: this test is alone in its binary.
    unsafe { std::env::set_var("PATH", &system) };
    let found = vodscribe_core::tools::find_ffmpeg(&bin).expect("an ffmpeg");
    assert_eq!(found, ffmpeg, "not the system's");
    let h265 = ClipEncoding { codec: ClipCodec::H265, quality: ClipQuality::Standard };
    let said = vodscribe_core::clips::can_make(&found, &bin, h265, true).unwrap_err().to_string();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(said, "Your system's ffmpeg can't make MP4 H.265. Pick another format in Settings › Encoding.");
}
