//! Programs faked for a test. `fake` puts a stand-in file at a program's
//! path (so `tools::find_ffmpeg` and the like find it) and registers an
//! answer for it: from then on `tools::spawn`, `output` and `status` run
//! that answer on a thread of this process instead of starting a process.
//! The answer reads the arguments the app gave, writes to the pipes the app
//! reads, and returns the exit code. Each fake is keyed by its path, so tests
//! running at once, each with its own folder, never meet.
//!
//! `Gate` lets a test hold a fake partway (ffmpeg half through a clip) and
//! act meanwhile, so a race is played in the order the test picks.

use std::ffi::OsString;
use std::io::{PipeReader, PipeWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
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
        let state = cv.wait_timeout_while(state.lock().unwrap(), Duration::from_secs(5), |s| s.arrived == 0).unwrap().0;
        assert!(state.arrived > 0, "no run reached the gate");
    }
}

/// A fake's run, as `tools::Child` sees it.
struct Fakery {
    thread: Option<JoinHandle<i32>>,
    stop: Arc<Stop>,
    status: Option<ExitStatus>,
    /// Second read ends of its stdout and stderr, emptied once it's stopped
    /// (`kill_tree`): an answer stuck writing to a full pipe the app no
    /// longer reads then ends, as a killed program would.
    outputs: Vec<PipeReader>,
}

impl Running for Fakery {
    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        // These let go of first, so an answer writing to a pipe the app has
        // let go of fails, as a program's write does.
        self.outputs.clear();
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
        *self.stop.stopped.lock().unwrap() = true;
        self.stop.changed.notify_all();
        // What it writes from now is read and dropped, here, not by the app,
        // which may still hold its ends without reading them.
        for mut output in self.outputs.drain(..) {
            std::thread::spawn(move || std::io::copy(&mut output, &mut std::io::sink()));
        }
        let _ = self.wait();
    }

    fn track(&self) -> Option<Tracked> {
        None
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
    let outputs = vec![stdout_read.try_clone()?, stderr_read.try_clone()?];
    let process = Fakery { thread: Some(thread), stop, status: None, outputs };
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
