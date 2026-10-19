//! The sound output going away while playing (a USB headset unplugged, a
//! device disabled): the time, which follows the sound's count, must go on.
//! Run by hand where an output can be taken away, with commands that do it:
//!
//!   VODSCRIBE_SOUND_OFF='<command>' VODSCRIBE_SOUND_ON='<command>' \
//!     cargo test -p vodscribe-player --test sound_loss -- --ignored --nocapture
//!
//! (Windows, as an administrator: Disable-PnpDevice / Enable-PnpDevice on the
//! speakers' AudioEndpoint, through powershell.)

use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use vodscribe_player::clock::{Event, Player};

fn shell(command: &str) {
    let status = if cfg!(windows) {
        Command::new("powershell").args(["-NoProfile", "-Command", command]).status()
    } else {
        Command::new("sh").args(["-c", command]).status()
    };
    assert!(status.is_ok_and(|s| s.success()), "{command} failed");
}

#[test]
#[ignore = "takes the sound output away: run by hand"]
fn the_time_goes_on_when_the_sound_output_goes() {
    let off = std::env::var("VODSCRIBE_SOUND_OFF").expect("VODSCRIBE_SOUND_OFF: a command taking the output away");
    let on = std::env::var("VODSCRIBE_SOUND_ON").expect("VODSCRIBE_SOUND_ON: a command bringing it back");
    let dir = std::env::temp_dir().join(format!("vodscribe-sound-loss-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("tone.mkv");
    let made = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "color=gray:s=160x90:r=30:d=20"])
        .args(["-f", "lavfi", "-i", "sine=f=440:d=20", "-c:v", "png", "-c:a", "pcm_s16le"])
        .arg(&file)
        .status()
        .unwrap();
    assert!(made.success());

    let player = Player::open_file(Path::new("ffmpeg"), &file).unwrap();
    let (tx, rx) = mpsc::channel();
    player.on_frame(Box::new(move |event| {
        let _ = tx.send(event);
    }));
    player.play();
    while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Running) {}
    assert!(player.has_sound(), "no sound output to take away");
    std::thread::sleep(Duration::from_secs(2));
    let before = player.time();
    println!("playing through an output, at {before:.2} s; taking it away");
    shell(&off);
    let took = Instant::now();
    std::thread::sleep(Duration::from_secs(4));
    let after = player.time();
    shell(&on);
    let moved = after - before;
    println!("{:.1} s later the time is {after:.2} s ({moved:.2} s on)", took.elapsed().as_secs_f64());
    drop(player);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(moved > 3.0, "the time stood when the sound output went: {moved:.2} s in 4 s");
}
