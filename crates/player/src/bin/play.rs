//! Plays a file in a bare window, without the app: the player as the app
//! has it (decode, clock, present), to run and look at on its own.
//!
//!   vodscribe-play <file> [ffmpeg]
//!   vodscribe-play --edl <file>:<start>-<end>[,<file>:<start>-<end>…] [ffmpeg]
//!   vodscribe-play --bench <file> [ffmpeg]
//!
//! `--edl` plays parts of files one after another, as an edit does.
//! ffmpeg is the one on PATH unless given. Space plays and pauses, the
//! arrows seek 5 s, J and L 10 s, comma and period step a frame, [ and ]
//! halve and double the speed, Home goes to the start, Q quits. The title
//! shows the time and frame, and a bar over the picture how far it's
//! played (drawn by the screen as the app's captions are); on quitting,
//! how long paints took is printed.
//!
//! `--bench` measures without a window: how fast the file decodes (frames
//! and megabytes a second, against its rate), how long seeks take to show
//! their frame, and over ten seconds of play how many frames are dropped
//! and how evenly they come.

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use std::path::PathBuf;
    use std::sync::Arc;

    use gtk::prelude::*;
    use vodscribe_player::Rect;
    use vodscribe_player::clock::Player;
    use vodscribe_player::present::Screen;

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
    // The screen covers the window's content, black underneath.
    let area = gtk::DrawingArea::new();
    area.connect_draw(|_, cr| {
        cr.set_source_rgb(0.0, 0.0, 0.0);
        let _ = cr.paint();
        glib::Propagation::Stop
    });
    window.add(&area);
    let screen = Screen::attach(area.upcast_ref());
    let placed = screen.clone();
    let size = Arc::new(std::sync::Mutex::new((0.0, 0.0)));
    let sized = size.clone();
    area.connect_size_allocate(move |_, a| {
        let rect = Rect { x: 0.0, y: 0.0, width: a.width() as f64, height: a.height() as f64 };
        *sized.lock().unwrap() = (rect.width, rect.height);
        placed.set(Some(rect));
    });
    let bar = std::sync::Mutex::new((0.0, 0.0, 0));

    // Each frame due is drawn, and the title brought up to date.
    let (shown, drawn) = (screen.clone(), player.clone());
    let title = glib::SendWeakRef::from(window.downgrade());
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    player.on_frame(Box::new(move |_| {
        shown.show(drawn.frame());
        let (time, fps, playing) = (drawn.time(), drawn.fps().unwrap_or(30.0), drawn.playing());
        if let Some(overlay) = progress(*size.lock().unwrap(), time / drawn.duration(), &bar) {
            shown.overlay(Some(overlay));
        }
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
    // A frame that came before the window listened (a start at VODSCRIBE_PLAY_AT).
    screen.show(player.frame());

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
            k::bracketleft => keys.set_speed(keys.speed() / 2.0),
            k::bracketright => keys.set_speed(keys.speed() * 2.0),
            k::q | k::Q | k::Escape => gtk::main_quit(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    window.connect_destroy(|_| gtk::main_quit());
    window.show_all();
    gtk::main();
    let (paints, took) = screen.painted();
    let (shown, dropped) = player.counts();
    if paints > 0 {
        println!(
            "paints {paints}, {:.2} ms each; frames shown {shown}, dropped {dropped}",
            took.as_secs_f64() * 1e3 / paints as f64
        );
    }
    Ok(())
}

/// A bar along the bottom of a box `size` (CSS pixels) a `fraction` of
/// the way filled, for the screen to draw over the picture; None when the
/// last one drawn (`last`: the box and the filled width) would do.
#[cfg(any(target_os = "linux", windows))]
fn progress(
    size: (f64, f64),
    fraction: f64,
    last: &std::sync::Mutex<(f64, f64, u32)>,
) -> Option<std::sync::Arc<vodscribe_player::Overlay>> {
    let (w, h) = ((size.0 - 32.0).max(1.0).round() as u32, 6u32);
    let filled = (fraction.clamp(0.0, 1.0) * w as f64).round() as u32;
    let mut last = last.lock().unwrap();
    if *last == (size.0, size.1, filled) {
        return None;
    }
    *last = (size.0, size.1, filled);
    // Premultiplied white: brighter where played.
    let pixels = (0..h)
        .flat_map(|_| (0..w).map(move |x| if x < filled { 230u8 } else { 77 }))
        .flat_map(|v| [v, v, v, v])
        .collect();
    let rect = vodscribe_player::Rect { x: 16.0, y: size.1 - 16.0 - h as f64, width: w as f64, height: h as f64 };
    Some(std::sync::Arc::new(vodscribe_player::Overlay::new(rect, (w, h), pixels)))
}

/// Stretches written `file:start-end`, separated by commas.
#[cfg(any(target_os = "linux", windows))]
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
        let Some(frame) = stream.recv() else { break };
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
    let began = Instant::now();
    while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Running) && player.has_sound() {}
    let waited = began.elapsed();
    // When each frame showed: how evenly they come.
    let mut shown = Vec::new();
    while began.elapsed() < Duration::from_secs(10) {
        if rx.recv_timeout(Duration::from_millis(50)).is_ok_and(|e| e == Event::Frame) {
            shown.push(Instant::now());
        }
    }
    player.pause();
    let after = player.counts();
    let mut gaps: Vec<f64> = shown.windows(2).map(|w| (w[1] - w[0]).as_secs_f64() * 1e3).collect();
    gaps.sort_by(f64::total_cmp);
    let at = |q: f64| gaps.get(((gaps.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0);
    println!(
        "play 10 s: {} frames shown, {} dropped (expected about {:.0}), time {:.2} s after {:.0} ms to start, sound {}; \
         frames {:.1} ms apart (median), 95% within {:.1} ms, longest {:.0} ms",
        after.0 - before.0,
        after.1 - before.1,
        10.0 * picture.fps,
        player.time(),
        waited.as_secs_f64() * 1e3,
        if player.has_sound() { "through an output" } else { "none" },
        at(0.5),
        at(0.95),
        at(1.0)
    );

    // Five seconds at each other speed, from the start: how far the time
    // goes against how far it should.
    for speed in [0.5, 2.0, 1.0] {
        player.set_speed(speed);
        player.seek(0.0);
        while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Frame) {}
        let before = player.counts();
        player.play();
        let began = Instant::now();
        while rx.recv_timeout(Duration::from_secs(10)).is_ok_and(|e| e != Event::Running) && player.has_sound() {}
        let started = player.time();
        let waited = began.elapsed().as_secs_f64();
        std::thread::sleep(Duration::from_secs(5));
        player.pause();
        let after = player.counts();
        println!(
            "play 5 s at {speed}x: time went {:.2} s (expected {:.2}) after {:.0} ms to start, {} frames shown, {} dropped",
            player.time() - started,
            5.0 * speed,
            waited * 1e3,
            after.0 - before.0,
            after.1 - before.1,
        );
    }
    Ok(())
}

/// On Windows: a bare window with the screen covering it; the same keys.
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use std::path::PathBuf;
    use std::sync::Arc;

    use vodscribe_player::Rect;
    use vodscribe_player::clock::Player;
    use vodscribe_player::present::Screen;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::w;

    let mut args = std::env::args_os().skip(1).peekable();
    let bench = args.next_if(|a| a == "--bench").is_some();
    let edl = args.next_if(|a| a == "--edl").is_some();
    let usage = || anyhow::anyhow!("usage: vodscribe-play [--bench | --edl] <file> [ffmpeg]");
    let file = PathBuf::from(args.next().ok_or_else(usage)?);
    let ffmpeg = args.next().map(PathBuf::from).unwrap_or_else(|| "ffmpeg".into());
    if bench {
        return measure(&ffmpeg, &file);
    }
    let player = if edl {
        Player::open(&ffmpeg, parse_edl(&file.to_string_lossy())?)?
    } else {
        Player::open_file(&ffmpeg, &file)?
    };
    if let Some(at) = std::env::var("VODSCRIBE_PLAY_AT").ok().and_then(|v| v.parse::<f64>().ok()) {
        player.seek(at);
    }
    let player = Arc::new(player);

    // The player and the screen, for the window procedure (one window).
    static PLAYER: std::sync::OnceLock<Arc<Player>> = std::sync::OnceLock::new();
    static SCREEN: std::sync::OnceLock<Screen> = std::sync::OnceLock::new();
    // The window's size in CSS pixels, for the bar over the picture.
    static SIZE: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 0.0));
    let _ = PLAYER.set(player.clone());

    unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_SIZE => {
                let (w, h) = ((lparam.0 & 0xffff) as f64, ((lparam.0 >> 16) & 0xffff) as f64);
                let scale = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd) } as f64 / 96.0;
                if let Some(screen) = SCREEN.get() {
                    let rect = Rect { x: 0.0, y: 0.0, width: w / scale, height: h / scale };
                    *SIZE.lock().unwrap() = (rect.width, rect.height);
                    screen.set(Some(rect));
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                if let Some(p) = PLAYER.get() {
                    let at = p.time();
                    match wparam.0 as u16 {
                        0x20 => p.toggle(),
                        0x25 => p.seek(at - 5.0),
                        0x27 => p.seek(at + 5.0),
                        0x4a => p.seek(at - 10.0),
                        0x4c => p.seek(at + 10.0),
                        0xbc => p.step(-1),
                        0xbe => p.step(1),
                        0x24 => p.seek(0.0),
                        0xdb => p.set_speed(p.speed() / 2.0),
                        0xdd => p.set_speed(p.speed() * 2.0),
                        0x51 | 0x1b => unsafe { PostQuitMessage(0) },
                        _ => {}
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: instance.into(),
            lpszClassName: w!("VODscribePlay"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("VODscribePlay"),
            w!("vodscribe-play"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            960,
            540,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let _ = SCREEN.set(Screen::attach(hwnd.0 as isize)?);
        // Each frame due is drawn, and the title brought up to date.
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let shown = hwnd.0 as isize;
        let bar = std::sync::Mutex::new((0.0, 0.0, 0));
        player.on_frame(Box::new(move |_| {
            let Some((screen, p)) = SCREEN.get().zip(PLAYER.get()) else { return };
            screen.show(p.frame());
            let (time, fps, playing) = (p.time(), p.fps().unwrap_or(30.0), p.playing());
            if let Some(overlay) = progress(*SIZE.lock().unwrap(), time / p.duration(), &bar) {
                screen.overlay(Some(overlay));
            }
            let frame = (time * fps + 1e-6).floor() as u64;
            let state = if playing { "" } else { " (paused)" };
            let title: Vec<u16> = format!("{name} - {time:.3} s, frame {frame}{state}\0").encode_utf16().collect();
            let _ = SetWindowTextW(HWND(shown as *mut _), windows::core::PCWSTR(title.as_ptr()));
        }));
        // A frame that came before the window listened (a start at VODSCRIBE_PLAY_AT).
        if let Some(screen) = SCREEN.get() {
            screen.show(player.frame());
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    let (shown, dropped) = player.counts();
    let (paints, took) = SCREEN.get().map_or((0, Default::default()), Screen::painted);
    println!(
        "paints {paints}, {:.2} ms each; frames shown {shown}, dropped {dropped}",
        took.as_secs_f64() * 1e3 / paints.max(1) as f64
    );
    Ok(())
}

/// Elsewhere (macOS): only `--bench`, which needs no window.
#[cfg(not(any(target_os = "linux", windows)))]
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let usage = || {
        anyhow::anyhow!(
            "usage: vodscribe-play --bench <file> [ffmpeg] (the window is for Linux and Windows only so far)"
        )
    };
    if args.next().is_none_or(|a| a != "--bench") {
        return Err(usage());
    }
    let file = std::path::PathBuf::from(args.next().ok_or_else(usage)?);
    let ffmpeg = args.next().map(std::path::PathBuf::from).unwrap_or_else(|| "ffmpeg".into());
    measure(&ffmpeg, &file)
}
