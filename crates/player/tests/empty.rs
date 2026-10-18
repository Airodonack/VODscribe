// An empty timeline (an edit with no pieces): play does nothing, and
// nothing runs flat out waiting for something to play.

use std::path::Path;
use std::time::Duration;

use vodscribe_player::clock::Player;
use vodscribe_player::timeline::Timeline;

/// This process's CPU time so far (user and system), from /proc.
#[cfg(target_os = "linux")]
fn cpu() -> Duration {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    // The fields after the name, which is in brackets and may hold spaces.
    let fields: Vec<&str> = stat.rsplit_once(')').unwrap().1.split_whitespace().collect();
    let ticks: u64 = fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap();
    // Clock ticks are 100 a second on Linux.
    Duration::from_millis(ticks * 10)
}

#[test]
fn an_empty_timeline_does_not_play() {
    let player = Player::open(Path::new("ffmpeg"), Timeline::default()).unwrap();
    #[cfg(target_os = "linux")]
    let before = cpu();
    player.play();
    std::thread::sleep(Duration::from_secs(1));
    #[cfg(target_os = "linux")]
    {
        let used = cpu() - before;
        assert!(used < Duration::from_millis(300), "{used:?} of CPU in a second, for nothing");
    }
    assert!(!player.playing(), "nothing to play: it stays paused");
}
