//! Programs faked for a test. `fake` puts a stand-in file at a program's
//! path (so `tools::find_ffmpeg` and the like find it) and registers an
//! answer for it: from then on `tools::spawn`, `output` and `status` run
//! that answer on a thread of this process instead of starting a process.
//! The answer reads the arguments the app gave, writes to the pipes the app
//! reads, and returns the exit code. Each fake is keyed by its path, so tests
//! running at once, each with its own folder, never meet.
//!
//! `Gate` lets a test hold a fake partway (ffmpeg half through a clip) and
//! act meanwhile, so a race is played in the order the test picks; and
//! `hold_at` holds the library's own code where no fake runs (a settings
//! file read, a save), by the file it's at.
//!
//! A run the app tracks (`tools::Child::track`), as it tracks a download or
//! an encode for its quit to stop, is tracked by the folder it works in, so
//! a test's quit stops only its own (`stop_tracked_in`).

use std::ffi::OsString;
use std::io::{PipeReader, PipeWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::tools::{Child, Running, Tracked};

/// What a fake does when it's run: given the run, returns its exit code.
pub type Answer = dyn Fn(&mut Run) -> i32 + Send + Sync;

/// Each fake: its path, its answer, and whether it's ended (`Fake::end`).
type Registered = (PathBuf, Arc<Answer>, Arc<AtomicBool>);

static FAKES: Mutex<Vec<Registered>> = Mutex::new(Vec::new());

/// A program faked at `path` until this is dropped (`fake`).
pub struct Fake {
    path: PathBuf,
    /// Its answer, so dropping it takes back only its own registration,
    /// not a later fake's at the same path.
    answer: Arc<Answer>,
    ended: Arc<AtomicBool>,
}

impl Fake {
    /// Where the program is faked.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Ends every run of it, now and to come, as if the app had stopped
    /// each: one held at a `Gate` lets go at once. For a test's end, so a
    /// test that fails while its fake is held ends rather than waits.
    pub fn end(&self) {
        self.ended.store(true, Ordering::Relaxed);
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.end();
        FAKES.lock().unwrap().retain(|(p, a, _)| !(p == &self.path && Arc::ptr_eq(a, &self.answer)));
    }
}

/// Fakes the program at `path` with `answer`, and puts an empty file there
/// so it's found (its folder made if need be).
pub fn fake(path: impl Into<PathBuf>, answer: impl Fn(&mut Run) -> i32 + Send + Sync + 'static) -> Fake {
    let path = path.into();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(&path, b"").unwrap();
    let mut fakes = FAKES.lock().unwrap();
    fakes.retain(|(p, _, _)| p != &path);
    let answer: Arc<Answer> = Arc::new(answer);
    let ended = Arc::new(AtomicBool::new(false));
    fakes.push((path.clone(), answer.clone(), ended.clone()));
    Fake { path, answer, ended }
}

fn answer_for(cmd: &Command) -> Option<(Arc<Answer>, Arc<AtomicBool>)> {
    let program = Path::new(cmd.get_program());
    FAKES.lock().unwrap().iter().find(|(p, _, _)| p == program).map(|(_, a, e)| (a.clone(), e.clone()))
}

/// One run of a fake: what the app gave it, and its pipes.
pub struct Run {
    args: Vec<OsString>,
    envs: Vec<(OsString, Option<OsString>)>,
    /// What the app writes to it; None once it's read whole.
    pub stdin: Option<PipeReader>,
    stdout: PipeWriter,
    stderr: PipeWriter,
    stop: Arc<Stop>,
    /// Its fake's end (`Fake::end`).
    ended: Arc<AtomicBool>,
}

impl Run {
    /// The arguments, as text.
    pub fn args(&self) -> Vec<String> {
        self.args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    /// The argument after the first `flag`, if any.
    pub fn after(&self, flag: &str) -> Option<String> {
        let args = self.args();
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
    }

    /// Whether `arg` is among the arguments.
    pub fn has(&self, arg: &str) -> bool {
        self.args.iter().any(|a| a == arg)
    }

    /// The variable `name` as the app set it for this run, if it did.
    pub fn env(&self, name: &str) -> Option<String> {
        self.envs.iter().rev().find(|(k, _)| k == name)?.1.as_ref().map(|v| v.to_string_lossy().into_owned())
    }

    /// Writes `text` to its stdout. Nothing once it's stopped, or once the
    /// app no longer reads it.
    pub fn out(&mut self, text: &str) {
        if !self.stopped() {
            let _ = self.stdout.write_all(text.as_bytes());
        }
    }

    /// `out`, as bytes.
    pub fn out_bytes(&mut self, bytes: &[u8]) {
        if !self.stopped() {
            let _ = self.stdout.write_all(bytes);
        }
    }

    /// Writes `text` to its stderr (`out`).
    pub fn err(&mut self, text: &str) {
        if !self.stopped() {
            let _ = self.stderr.write_all(text.as_bytes());
        }
    }

    /// Whether the app has stopped it (`tools::Child::kill_tree`). An answer
    /// that does more than a moment's work checks, and returns when it is.
    pub fn stopped(&self) -> bool {
        *self.stop.stopped.lock().unwrap() || self.ended.load(Ordering::Relaxed)
    }

    /// Waits at `gate` till the test opens it; false if the app stopped the
    /// run meanwhile.
    pub fn hold(&self, gate: &Gate) -> bool {
        gate.arrive();
        let mut stopped = self.stop.stopped.lock().unwrap();
        loop {
            if *stopped || self.ended.load(Ordering::Relaxed) {
                return false;
            }
            if gate.is_open() {
                return true;
            }
            // Woken by a stop; the gate's opening and the fake's end are
            // seen within a moment.
            stopped = self.stop.changed.wait_timeout(stopped, Duration::from_millis(2)).unwrap().0;
        }
    }
}

/// Whether the app has stopped a run, and its news.
#[derive(Default)]
struct Stop {
    stopped: Mutex<bool>,
    changed: Condvar,
    /// Second read ends of its stdout and stderr, emptied once it's stopped
    /// (`stop`): an answer stuck writing to a full pipe the app no longer
    /// reads then ends, as a killed program would.
    outputs: Mutex<Vec<PipeReader>>,
}

impl Stop {
    /// Stops the run, as killing a program does (`tools::kill_tree`).
    fn stop(&self) {
        *self.stopped.lock().unwrap() = true;
        self.changed.notify_all();
        // What it writes from now is read and dropped, here, not by the app,
        // which may still hold its ends without reading them.
        for mut output in self.outputs.lock().unwrap().drain(..) {
            std::thread::spawn(move || std::io::copy(&mut output, &mut std::io::sink()));
        }
    }
}

/// A point a fake waits at (`Run::hold`) until the test opens it. Clones
/// are the same gate.
#[derive(Clone, Default)]
pub struct Gate(Arc<(Mutex<GateState>, Condvar)>);

#[derive(Default)]
struct GateState {
    open: bool,
    arrived: usize,
}

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }

    /// A gate open from the start: given in place of one something was
    /// held at, it holds nothing more.
    pub fn opened() -> Self {
        let gate = Self::new();
        gate.open();
        gate
    }

    /// Lets every run waiting here, and any to come, go on.
    pub fn open(&self) {
        self.0.0.lock().unwrap().open = true;
        self.0.1.notify_all();
    }

    fn is_open(&self) -> bool {
        self.0.0.lock().unwrap().open
    }

    fn arrive(&self) {
        self.0.0.lock().unwrap().arrived += 1;
        self.0.1.notify_all();
    }

    /// Waits here till the test opens the gate, counted as come to it
    /// (`reached`): for a seam in the app's own code, where no fake runs.
    pub fn wait(&self) {
        self.arrive();
        let (state, cv) = &*self.0;
        drop(cv.wait_while(state.lock().unwrap(), |s| !s.open).unwrap());
    }

    /// Waits till a run has come to the gate (up to five seconds; panics if
    /// none does): the moment the test acts in.
    pub fn reached(&self) {
        let (state, cv) = &*self.0;
        let waited = cv.wait_timeout_while(state.lock().unwrap(), Duration::from_secs(5), |s| s.arrived == 0);
        // Its lock let go of first: a panic holding it would poison the
        // gate, and the app's end, opening it, would panic again and abort.
        let arrived = waited.unwrap().0.arrived;
        assert!(arrived > 0, "no run reached the gate");
    }
}

/// A point in the library's own code a test holds it at (`hold_at`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum At {
    /// `Settings::load`: a settings file read and found unreadable, before
    /// it's set aside.
    SettingsRead,
    /// `Settings::save`: asked, before it takes its turn (and writes), so
    /// a save asked later can go first.
    SettingsSave,
    /// A check for VODscriptions (`annotations_filter`), for a settings
    /// file: asking found on, before it waits its turn behind a check
    /// under way.
    CheckTurn,
    /// A check for VODscriptions, for a settings file: the pause between
    /// two VODs' requests over, before it reads whether asking is on.
    CheckPause,
}

/// The gates held at points of the library's own code (`hold_at`), with
/// the file each is for: tests run at once, in one process.
static HELD: Mutex<Vec<(At, PathBuf, Gate)>> = Mutex::new(Vec::new());

fn held() -> MutexGuard<'static, Vec<(At, PathBuf, Gate)>> {
    HELD.lock().unwrap_or_else(|e| e.into_inner())
}

/// From now, the code at `at` for the file at `path` waits at `gate` till
/// the test opens it, in place of any gate it waited at there before.
pub fn hold_at(at: At, path: &Path, gate: &Gate) {
    let mut held = held();
    held.retain(|(a, p, _)| !(*a == at && p == path));
    held.push((at, path.to_path_buf(), gate.clone()));
}

/// The code at `at` for the file at `path` has come there: it waits if a
/// test holds it there (`hold_at`).
pub fn held_at(at: At, path: &Path) {
    let gate = held().iter().find(|(a, p, _)| *a == at && p == path).map(|(_, _, gate)| gate.clone());
    if let Some(gate) = gate {
        gate.wait();
    }
}

/// Opens and forgets each gate held for a file in `dir` (`hold_at`), and
/// forgets that a quit stopped the runs there for good (`stop_tracked_in`)
/// and the PATH given to a folder there (`path_for`): a test's end, so
/// nothing it held waits on, and the next app started on its folder runs.
pub fn let_go(dir: &Path) {
    held().retain(|(_, path, gate)| {
        let ours = path.starts_with(dir);
        if ours {
            gate.open();
        }
        !ours
    });
    tracking().quit.retain(|quit| !quit.starts_with(dir));
    paths().retain(|(bin, _)| !bin.starts_with(dir));
}

/// The PATH each test gave the app whose programs are in a folder
/// (`path_for`): the tests run at once, in one process, which has one PATH.
fn paths() -> MutexGuard<'static, Vec<(PathBuf, Vec<PathBuf>)>> {
    static PATHS: Mutex<Vec<(PathBuf, Vec<PathBuf>)>> = Mutex::new(Vec::new());
    PATHS.lock().unwrap_or_else(|e| e.into_inner())
}

/// From now, the app whose own programs are in `bin_dir` (`Paths::bin`)
/// finds the system's (`tools::on_path`: yt-dlp, ffmpeg, a JavaScript
/// runtime, nvidia-smi) in `dirs` alone, as if they were its PATH, not
/// this computer's: none at all for none. Until `let_go` of a folder it's
/// in.
pub fn path_for(bin_dir: &Path, dirs: &[PathBuf]) {
    let mut paths = paths();
    paths.retain(|(bin, _)| bin != bin_dir);
    paths.push((bin_dir.to_path_buf(), dirs.to_vec()));
}

/// The PATH a test gave the app whose programs are in `bin_dir`, if one did.
pub(crate) fn path_of(bin_dir: &Path) -> Option<Vec<PathBuf>> {
    paths().iter().find(|(bin, _)| bin == bin_dir).map(|(_, dirs)| dirs.clone())
}

/// The runs the app tracks, as `tools::stop_tracked` keeps the processes.
struct Tracking {
    /// Each by its number: where it works (`Fakery::works_in`) and its stop.
    runs: Vec<(u64, Vec<PathBuf>, Arc<Stop>)>,
    /// The folders a quit stopped the runs of for good: one tracked there
    /// from then on is stopped at once.
    quit: Vec<PathBuf>,
    /// The next run's number.
    next: u64,
}

fn tracking() -> MutexGuard<'static, Tracking> {
    static TRACKING: Mutex<Tracking> = Mutex::new(Tracking { runs: Vec::new(), quit: Vec::new(), next: 0 });
    TRACKING.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether a run working at `paths` works in `dir`.
fn works_in(paths: &[PathBuf], dir: &Path) -> bool {
    paths.iter().any(|path| path.starts_with(dir))
}

/// Stops each run the app tracks (`tools::Child::track`) that works in
/// `dir`, its program or a path among its arguments there, and every one
/// tracked there from now: what `tools::stop_tracked` does to processes as
/// the app quits, for a test's quit, each test stopping only its own. Runs
/// not tracked go on, as processes do.
pub fn stop_tracked_in(dir: &Path) {
    let stopped: Vec<Arc<Stop>> = {
        // Under one lock with `track`'s: each run is taken here or tracked
        // once the folder's quit is known, and so stopped there.
        let mut tracking = tracking();
        tracking.quit.push(dir.to_path_buf());
        let (ours, others) = std::mem::take(&mut tracking.runs).into_iter().partition(|(_, at, _)| works_in(at, dir));
        tracking.runs = others;
        ours.into_iter().map(|(_, _, stop)| stop).collect()
    };
    for stop in stopped {
        stop.stop();
    }
}

/// Untracks run `run` (`tools::Tracked`, dropped).
pub(crate) fn untrack(run: u64) {
    tracking().runs.retain(|(n, _, _)| *n != run);
}

/// A fake's run, as `tools::Child` sees it.
struct Fakery {
    thread: Option<JoinHandle<i32>>,
    stop: Arc<Stop>,
    status: Option<ExitStatus>,
    /// Where it works: its program, and its arguments as paths.
    works_in: Vec<PathBuf>,
}

impl Running for Fakery {
    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        // These let go of first, so an answer writing to a pipe the app has
        // let go of fails, as a program's write does.
        self.stop.outputs.lock().unwrap().clear();
        // A panic in the answer (a test's assert) ends it as a crash would.
        let code = self.thread.take().map_or(0, |t| t.join().unwrap_or(101));
        let status = exit_status(code, *self.stop.stopped.lock().unwrap());
        self.status = Some(status);
        Ok(status)
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        match &self.thread {
            Some(t) if !t.is_finished() => Ok(None),
            _ => self.wait().map(Some),
        }
    }

    fn kill_tree(&mut self) {
        self.stop.stop();
        let _ = self.wait();
    }

    fn track(&self) -> Option<Tracked> {
        let mut tracking = tracking();
        let run = tracking.next;
        tracking.next += 1;
        if tracking.quit.iter().any(|dir| works_in(&self.works_in, dir)) {
            self.stop.stop();
        } else {
            tracking.runs.push((run, self.works_in.clone(), self.stop.clone()));
        }
        Some(Tracked::fake(run))
    }
}

/// The status a program ending with `code`, or stopped (`killed`), has.
fn exit_status(code: i32, killed: bool) -> ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        // SIGKILL's, as `kill_tree` ends a program.
        if killed { ExitStatus::from_raw(9) } else { ExitStatus::from_raw((code & 0xff) << 8) }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(if killed { 1 } else { code as u32 })
    }
}

/// `cmd` run as its program's fake, if it has one (`tools::spawn`).
pub(crate) fn spawn(cmd: &mut Command) -> Option<std::io::Result<Child>> {
    let answer = answer_for(cmd)?;
    Some(start(cmd, answer))
}

fn start(cmd: &Command, (answer, ended): (Arc<Answer>, Arc<AtomicBool>)) -> std::io::Result<Child> {
    let (stdin_read, stdin_write) = std::io::pipe()?;
    let (stdout_read, stdout_write) = std::io::pipe()?;
    let (stderr_read, stderr_write) = std::io::pipe()?;
    let stop = Arc::new(Stop::default());
    let mut run = Run {
        args: cmd.get_args().map(|a| a.to_os_string()).collect(),
        envs: cmd.get_envs().map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string()))).collect(),
        stdin: Some(stdin_read),
        stdout: stdout_write,
        stderr: stderr_write,
        stop: stop.clone(),
        ended,
    };
    let name = Path::new(cmd.get_program()).file_name().map(|n| n.to_string_lossy().into_owned());
    let thread = std::thread::Builder::new()
        .name(format!("fake {}", name.unwrap_or_default()))
        .spawn(move || answer(&mut run))?;
    *stop.outputs.lock().unwrap() = vec![stdout_read.try_clone()?, stderr_read.try_clone()?];
    let works_in = std::iter::once(cmd.get_program()).chain(cmd.get_args()).map(PathBuf::from).collect();
    let process = Fakery { thread: Some(thread), stop, status: None, works_in };
    Ok(Child::new(
        Some(Box::new(stdin_write)),
        Some(Box::new(stdout_read)),
        Some(Box::new(stderr_read)),
        Box::new(process),
    ))
}

/// `cmd.output()` from its program's fake, if it has one (`tools::output`).
pub(crate) fn output(cmd: &mut Command) -> Option<std::io::Result<Output>> {
    let answer = answer_for(cmd)?;
    Some((|| {
        let mut child = start(cmd, answer)?;
        child.stdin = None;
        let stderr = child.stderr.take().map(crate::tools::drain);
        let mut stdout = Vec::new();
        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_end(&mut stdout)?;
        }
        let status = child.wait()?;
        let stderr = stderr.map(|t| t.join().unwrap_or_default()).unwrap_or_default();
        Ok(Output { status, stdout, stderr: stderr.into_bytes() })
    })())
}
