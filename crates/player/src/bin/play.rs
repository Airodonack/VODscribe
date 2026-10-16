//! Plays a file in a bare window, without the app: the player as the app
//! has it (decode, clock, present), to run and look at on its own.
//!
//!   vodscribe-play <file> [ffmpeg]
//!   vodscribe-play --edl <file>:<start>-<end>[,<file>:<start>-<end>…] [ffmpeg]
//!   vodscribe-play --bench <file> [ffmpeg]
//!
//! `--edl` plays parts of files one after another, as an edit does.
//! ffmpeg is the one on PATH unless given. Space plays and pauses, the
//! arrows seek 5 s, J and L 10 s, comma and period step a frame, Home goes
//! to the start, Q quits. The title shows the time and frame; on quitting,
//! how long paints took is printed.
//!
//! `--bench` measures without a window: how fast the file decodes (frames
//! and megabytes a second, against its rate), how long seeks take to show
//! their frame, and how many frames are dropped over ten seconds of play.

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use std::path::PathBuf;
    use std::sync::Arc;

    use gtk::prelude::*;
    use vodscribe_player::clock::Player;
    use vodscribe_player::present::{Canvas, Place};

    let mut args = std::env::args_os().skip(1).peekable();
    let bench = args.next_if(|a| a == "--bench").is_some();
    let edl = args.next_if(|a| a == "--edl").is_some();
    let usage = || anyhow::anyhow!("usage: vodscribe-play [--bench | --edl] <file> [ffmpeg]");
    let file = PathBuf::from(args.next().ok_or_else(usage)?);
    let ffmpeg = args.next().map(PathBuf::from).unwrap_or_else(|| "ffmpeg".into());
    if bench {
        return measure(&ffmpeg, &file);
    }

    gtk::init()?;
    let player = if edl {
        Player::open(&ffmpeg, parse_edl(&file.to_string_lossy())?)?
    } else {
        Player::open_file(&ffmpeg, &file)?
    };
    // VODSCRIBE_PLAY_AT=<seconds> starts there, paused (for scripted checks).
    if let Some(at) = std::env::var("VODSCRIBE_PLAY_AT").ok().and_then(|v| v.parse::<f64>().ok()) {
        player.seek(at);
    }
    let player = Arc::new(player);
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(960, 540);
    let area = gtk::GLArea::new();
    let canvas = Canvas::new(&area, Place::Over);
    window.add(&area);

    // Each frame due is drawn, and the title brought up to date.
    let (shown, drawn) = (canvas.clone(), player.clone());
    let title = glib::SendWeakRef::from(window.downgrade());
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    player.on_frame(Box::new(move |_| {
        shown.show(drawn.frame());
        let (time, fps, playing) = (drawn.time(), drawn.fps().unwrap_or(30.0), drawn.playing());
        let title = title.clone();
        let name = name.clone();
        glib::MainContext::default().invoke(move || {
            if let Some(window) = title.upgrade() {
                let frame = (time * fps + 1e-6).floor() as u64;
                let state = if playing { "" } else { " (paused)" };
                window.set_title(&format!("{name} - {time:.3} s, frame {frame}{state}"));
            }
        });
    }));

    let keys = player.clone();
    window.connect_key_press_event(move |_, key| {
        use gtk::gdk::keys::constants as k;
        let at = keys.time();
        match key.keyval() {
            k::space => keys.toggle(),
            k::Left => keys.seek(at - 5.0),
            k::Right => keys.seek(at + 5.0),
            k::j | k::J => keys.seek(at - 10.0),
            k::l | k::L => keys.seek(at + 10.0),
            k::comma => keys.step(-1),
            k::period => keys.step(1),
            k::Home => keys.seek(0.0),
            k::q | k::Q | k::Escape => gtk::main_quit(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    window.connect_destroy(|_| gtk::main_quit());
    window.show_all();
    gtk::main();
    let (paints, took) = canvas.painted();
    let (shown, dropped) = player.counts();
    if paints > 0 {
        println!(
            "paints {paints}, {:.2} ms each; frames shown {shown}, dropped {dropped}",
            took.as_secs_f64() * 1e3 / paints as f64
        );
    }
    Ok(())
}

/// Stretches written `file:start-end`, separated by commas.
#[cfg(target_os = "linux")]
fn parse_edl(text: &str) -> anyhow::Result<vodscribe_player::timeline::Timeline> {
    use vodscribe_core::footage::Source;
    use vodscribe_player::timeline::{Stretch, Timeline};
    let mut stretches = Vec::new();
    let mut at = 0.0;
    for part in text.split(',') {
        let (file, range) = part.rsplit_once(':').ok_or_else(|| anyhow::anyhow!("{part}: not file:start-end"))?;
        let (start, end) = range.split_once('-').ok_or_else(|| anyhow::anyhow!("{part}: not file:start-end"))?;
        let (start, end): (f64, f64) = (start.parse()?, end.parse()?);
        let source = Some(Source { path: file.into(), offset: 0.0, section: None });
        stretches.push(Stretch { at, length: end - start, piece_id: None, source, from: start });
        at += end - start;
    }
    Ok(Timeline::from_stretches(stretches))
}

/// Measures the player on `file`, printing what it finds.
#[cfg(target_os = "linux")]
fn measure(ffmpeg: &std::path::Path, file: &std::path::Path) -> anyhow::Result<()> {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use vodscribe_player::clock::{Event, Player};
    use vodscribe_player::decode::{self, Video};

    let info = decode::probe(ffmpeg, file)?;
    let picture = info.picture.ok_or_else(|| anyhow::anyhow!("no picture in the file"))?;
    let duration = info.duration.unwrap_or(0.0);
    println!("{}x{} at {} fps, {duration:.1} s", picture.width, picture.height, picture.fps);

    // Decoding alone, as fast as frames can be taken.
    let video = Video::new(ffmpeg, file, picture);
    let mut stream = video.from(0.0)?;
    let began = Instant::now();
    let (mut frames, mut bytes) = (0u64, 0u64);
    while began.elapsed() < Duration::from_secs(5) {
        let Some(frame) = stream.next() else { break };
        frames += 1;
        bytes += frame.rgb.len() as u64;
    }
    let secs = began.elapsed().as_secs_f64();
    println!(
        "decode: {:.0} frames/s ({:.1}x the rate), {:.0} MB/s over the pipe",
        frames as f64 / secs,
        frames as f64 / secs / picture.fps,
        bytes as f64 / secs / 1e6
    );
    drop(stream);

    // Seeks, paused: from asking to the frame there showing.
    let player = Player::open_file(ffmpeg, file)?;
    let (tx, rx) = mpsc::channel();
    player.on_frame(Box::new(move |event| {
        let _ = tx.send(event);
    }));
    while rx.recv_timeout(Duration::from_secs(5)).is_ok_and(|e| e != Event::Frame) {}
    let mut times = Vec::new();
    for i in 0..10 {
        let at = duration * (i as f64 + 0.5) / 10.0;
        while rx.try_recv().is_ok() {}
        let began = Instant::now();
        player.seek(at);
        while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Frame) {}
        times.push(began.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(|a, b| a.total_cmp(b));
    println!("seek to frame: {:.0} ms least, {:.0} ms median, {:.0} ms most", times[0], times[5], times[9]);

    // Ten seconds of play from the start.
    player.seek(0.0);
    while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Frame) {}
    let before = player.counts();
    player.play();
    std::thread::sleep(Duration::from_secs(10));
    player.pause();
    let after = player.counts();
    println!(
        "play 10 s: {} frames shown, {} dropped (expected about {:.0}), time {:.2} s, sound {}",
        after.0 - before.0,
        after.1 - before.1,
        10.0 * picture.fps,
        player.time(),
        if player.has_sound() { "through an output" } else { "none" }
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The player's window is built for Linux only so far.");
}
