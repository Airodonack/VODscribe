// A site's first segment as Twitch's player hands it over: H.264 with
// B-frames and no edit list, so it's decoded from 0 but its first frame is
// shown 2 frames on. Their player starts where the picture and sound both
// begin as it reckons them from the embed shim's buffered ranges. Those
// once said the picture's decode times (from 0), so it started at the
// sound's 0.035, before the first shown frame; they now say when frames
// show, as a browser's do. The case stays: a start between a run's first
// decode time and its first shown frame must still play.

use std::path::Path;
use std::time::{Duration, Instant};

use vodscribe_core::footage::Source;
use vodscribe_player::clock::Player;
use vodscribe_player::timeline::{Sound, Stretch, Timeline};

#[test]
fn played_from_before_its_first_shown_frame_it_plays() {
    let dir = std::env::temp_dir().join(format!("vodscribe-shown-later-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("v.mp4");
    let made = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=4"])
        .args(["-c:v", "libx264", "-bf", "2", "-use_editlist", "0"])
        .args(["-movflags", "frag_keyframe+empty_moov+default_base_moof"])
        .arg(&video)
        .status();
    // Its sound apart, from 0.035 (Twitch's AAC starts there).
    let audio = dir.join("a.mp4");
    let made = made.and_then(|_| {
        std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=duration=4", "-c:a", "aac"])
            .args(["-output_ts_offset", "0.035", "-movflags", "frag_keyframe+empty_moov+default_base_moof"])
            .arg(&audio)
            .status()
    });
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    }
    // As embed.rs lays them out: nothing till both are, then both.
    let gap = Stretch { at: 0.0, length: 0.035, piece_id: None, source: None, from: 0.0, sound: None };
    let timeline = Timeline::from_stretches(vec![
        gap,
        Stretch {
            at: 0.035,
            length: 3.9,
            piece_id: None,
            source: Some(Source { path: video, offset: 0.0, section: None }),
            from: 0.035,
            sound: Some(Sound { path: audio, offset: 0.035 }),
        },
    ]);
    let player = Player::open(Path::new("ffmpeg"), timeline).unwrap();
    // As a site's is: in a gap, it waits for footage to come.
    player.set_waits_for_footage(true);
    // Between its first decode time (0) and its first shown frame (0.067).
    player.seek(0.035);
    player.play();
    let began = Instant::now();
    while player.time() < 1.0 {
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "it didn't play: at {} s, running {}, waiting for footage {}",
            player.time(),
            player.running(),
            player.waiting_for_footage()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(dir);
}
