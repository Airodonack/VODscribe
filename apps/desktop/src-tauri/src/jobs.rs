//! Work done one job at a time, in the order asked, on a thread of its own:
//! clips cut (clips.rs), edits rendered (edits.rs), videos downloaded
//! (videos.rs), storyboards made (storyboards.rs). A queue keeps what's
//! waiting and what's running with its stop, so a job can be taken out
//! before its turn, or stopped during it, or put in another order; and it
//! says when what it holds changes (`watch`), for the list of jobs the
//! library shows (job_list.rs).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use anyhow::Result;

struct Queue<J> {
    waiting: VecDeque<J>,
    running: Option<(J, Arc<AtomicBool>)>,
    /// Ended (`close`): its thread stops.
    closed: bool,
}

/// What's told of each change to a queue (`Jobs::watch`).
type Watcher = Arc<OnceLock<Box<dyn Fn() + Send + Sync>>>;

pub(crate) struct Jobs<J> {
    shared: Arc<(Mutex<Queue<J>>, Condvar)>,
    started: AtomicBool,
    watcher: Watcher,
    /// Its thread, for `close` to wait for.
    #[cfg(test)]
    thread: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl<J: Clone + Send + 'static> Jobs<J> {
    pub(crate) fn new() -> Self {
        let queue = Queue { waiting: VecDeque::new(), running: None, closed: false };
        Jobs {
            shared: Arc::new((Mutex::new(queue), Condvar::new())),
            started: AtomicBool::new(false),
            watcher: Arc::default(),
            #[cfg(test)]
            thread: Mutex::default(),
        }
    }

    /// Calls `changed` after each change to what's waiting or running: a job
    /// added, taken out, begun, ended or moved. With the queue let go of, so
    /// it may read it; it shouldn't wait on anything a caller may hold (the
    /// library's lock): job_list.rs only sends word to a thread of its own.
    pub(crate) fn watch(&self, changed: impl Fn() + Send + Sync + 'static) {
        let _ = self.watcher.set(Box::new(changed));
    }

    fn changed(&self) {
        tell(&self.watcher);
    }

    /// Adds `job` last.
    pub(crate) fn push(&self, job: J) {
        let (queue, wake) = &*self.shared;
        queue.lock().unwrap().waiting.push_back(job);
        wake.notify_one();
        self.changed();
    }

    /// Adds `job` last unless one `same` finds is waiting already; whether it did.
    pub(crate) fn push_once(&self, job: J, same: impl Fn(&J) -> bool) -> bool {
        let (queue, wake) = &*self.shared;
        let mut queue = queue.lock().unwrap();
        if queue.waiting.iter().any(same) {
            return false;
        }
        queue.waiting.push_back(job);
        wake.notify_one();
        drop(queue);
        self.changed();
        true
    }

    /// Adds `job` last unless one `same` finds is waiting or running (and not
    /// stopped); whether it did.
    pub(crate) fn push_new(&self, job: J, same: impl Fn(&J) -> bool) -> bool {
        let (queue, wake) = &*self.shared;
        let mut queue = queue.lock().unwrap();
        if running_on(&queue.running, &same) || queue.waiting.iter().any(same) {
            return false;
        }
        queue.waiting.push_back(job);
        wake.notify_one();
        drop(queue);
        self.changed();
        true
    }

    /// Puts `job` first: the one `same` finds waiting is moved there, or
    /// `job` is added there unless one is running (and not stopped: one being
    /// stopped is ending, and a job asked for now is a new one, such as a
    /// clip given a deleted one's id while its cut winds down); whether it
    /// was added.
    pub(crate) fn push_front(&self, job: J, same: impl Fn(&J) -> bool) -> bool {
        let (queue, wake) = &*self.shared;
        let mut queue = queue.lock().unwrap();
        if let Some(at) = queue.waiting.iter().position(&same) {
            let waiting = queue.waiting.remove(at).expect("found");
            queue.waiting.push_front(waiting);
            drop(queue);
            self.changed();
            return false;
        }
        if running_on(&queue.running, &same) {
            return false;
        }
        queue.waiting.push_front(job);
        wake.notify_one();
        drop(queue);
        self.changed();
        true
    }

    /// Takes the jobs `which` finds out of those waiting, and returns them.
    pub(crate) fn take_waiting(&self, which: impl Fn(&J) -> bool) -> Vec<J> {
        let mut queue = self.shared.0.lock().unwrap();
        let (taken, kept): (VecDeque<J>, VecDeque<J>) = queue.waiting.drain(..).partition(|j| which(j));
        queue.waiting = kept;
        drop(queue);
        if !taken.is_empty() {
            self.changed();
        }
        taken.into()
    }

    /// Puts the jobs waiting that `order` names in its order, in the places
    /// they hold among those waiting; the others (added since the order was
    /// read) keep theirs. The one running is begun, and stays.
    pub(crate) fn reorder<K: PartialEq>(&self, order: &[K], key: impl Fn(&J) -> K) {
        let mut queue = self.shared.0.lock().unwrap();
        let rank = |j: &J| order.iter().position(|k| *k == key(j));
        let places: Vec<usize> = (0..queue.waiting.len()).filter(|&at| rank(&queue.waiting[at]).is_some()).collect();
        let mut named: Vec<J> = places.iter().map(|&at| queue.waiting[at].clone()).collect();
        named.sort_by_key(|j| rank(j));
        let moved = places.iter().zip(&named).any(|(&at, j)| rank(&queue.waiting[at]) != rank(j));
        for (&at, j) in places.iter().zip(named) {
            queue.waiting[at] = j;
        }
        drop(queue);
        if moved {
            self.changed();
        }
    }

    /// Whether a job `which` finds is waiting.
    pub(crate) fn is_waiting(&self, which: impl Fn(&J) -> bool) -> bool {
        self.shared.0.lock().unwrap().waiting.iter().any(which)
    }

    /// Whether a job `which` finds is waiting, or running and not stopped: one
    /// that will still write what it makes.
    pub(crate) fn is_wanted(&self, which: impl Fn(&J) -> bool) -> bool {
        let queue = self.shared.0.lock().unwrap();
        let running = queue.running.as_ref().is_some_and(|(j, stop)| !stop.load(Ordering::Relaxed) && which(j));
        running || queue.waiting.iter().any(which)
    }

    /// Stops the job running if it's one `which` finds; whether it was.
    pub(crate) fn stop_running(&self, which: impl Fn(&J) -> bool) -> bool {
        let queue = self.shared.0.lock().unwrap();
        match &queue.running {
            Some((job, stop)) if which(job) => {
                stop.store(true, Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }

    /// Stops the job running if it's one `which` finds, and calls `mark`
    /// with it, under the queue's lock; whether it was. Its end (`run_ended`'s
    /// `ended`) comes after, never between: what `mark` notes, `ended` finds.
    pub(crate) fn stop_running_and_mark(&self, which: impl Fn(&J) -> bool, mark: impl FnOnce(&J)) -> bool {
        let queue = self.shared.0.lock().unwrap();
        match &queue.running {
            Some((job, stop)) if which(job) => {
                stop.store(true, Ordering::Relaxed);
                mark(job);
                true
            }
            _ => false,
        }
    }

    /// Stops the job running if it's one `which` finds, and waits up to
    /// `timeout` for it to end; whether none is running now. Not from a job.
    pub(crate) fn stop_and_wait(&self, which: impl Fn(&J) -> bool, timeout: std::time::Duration) -> bool {
        let (queue, wake) = &*self.shared;
        let deadline = std::time::Instant::now() + timeout;
        let mut queue = queue.lock().unwrap();
        loop {
            match &queue.running {
                Some((job, stop)) if which(job) => stop.store(true, Ordering::Relaxed),
                _ => return true,
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return false;
            }
            queue = wake.wait_timeout(queue, left).unwrap().0;
        }
    }

    /// The job running, if one is, and those waiting, in order.
    pub(crate) fn snapshot(&self) -> (Option<J>, Vec<J>) {
        let queue = self.shared.0.lock().unwrap();
        (queue.running.as_ref().map(|(j, _)| j.clone()), queue.waiting.iter().cloned().collect())
    }

    /// The job running, then those waiting, in order.
    pub(crate) fn listed(&self) -> Vec<J> {
        let queue = self.shared.0.lock().unwrap();
        queue.running.iter().map(|(j, _)| j.clone()).chain(queue.waiting.iter().cloned()).collect()
    }

    /// Starts the thread (`name`) doing each job with `work`, given its stop.
    pub(crate) fn run(&self, name: &str, work: impl Fn(J, &AtomicBool) + Send + 'static) -> Result<()> {
        self.run_ended(name, work, |_| (), |_, ()| {})
    }

    /// As `run`, and as each job ends, asks `ended` about it under the
    /// queue's lock, as the job stops being the one running: a
    /// `stop_running_and_mark` comes wholly before that or finds no job. What
    /// `ended` says goes to `after_unlock`, once the lock is let go of, for
    /// work that takes other locks (the library's is taken before this one:
    /// `Cutter::stop_vod`).
    pub(crate) fn run_ended<T>(
        &self,
        name: &str,
        work: impl Fn(J, &AtomicBool) + Send + 'static,
        ended: impl Fn(&J) -> T + Send + 'static,
        after_unlock: impl Fn(J, T) + Send + 'static,
    ) -> Result<()> {
        // A second thread would run two jobs at once, and `running` holds one.
        let again = self.started.swap(true, Ordering::Relaxed);
        debug_assert!(!again, "the {name} queue's thread was started twice");
        if again {
            return Ok(());
        }
        let shared = self.shared.clone();
        let watcher = self.watcher.clone();
        let thread = std::thread::Builder::new().name(name.into()).spawn(move || {
            let (queue, wake) = &*shared;
            loop {
                let (job, stop) = {
                    let mut queue = queue.lock().unwrap();
                    let job = loop {
                        if queue.closed {
                            return;
                        }
                        if let Some(job) = queue.waiting.pop_front() {
                            break job;
                        }
                        queue = wake.wait(queue).unwrap();
                    };
                    let stop = Arc::new(AtomicBool::new(false));
                    queue.running = Some((job.clone(), stop.clone()));
                    (job, stop)
                };
                tell(&watcher);
                work(job.clone(), &stop);
                let said = {
                    let mut queue = queue.lock().unwrap();
                    queue.running = None;
                    ended(&job)
                };
                // For `stop_and_wait`.
                wake.notify_all();
                after_unlock(job, said);
                tell(&watcher);
            }
        })?;
        #[cfg(test)]
        {
            *self.thread.lock().unwrap() = Some(thread);
        }
        #[cfg(not(test))]
        drop(thread);
        Ok(())
    }

    /// Ends the queue for good: what's waiting is dropped, the job running
    /// is stopped, and its thread ends; returns once it has. For the tests'
    /// app (`testkit`), which ends with its test.
    #[cfg(test)]
    pub(crate) fn close(&self) {
        let (queue, wake) = &*self.shared;
        {
            let mut queue = queue.lock().unwrap();
            queue.closed = true;
            queue.waiting.clear();
            if let Some((_, stop)) = &queue.running {
                stop.store(true, Ordering::Relaxed);
            }
        }
        wake.notify_all();
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
}

fn tell(watcher: &Watcher) {
    if let Some(changed) = watcher.get() {
        changed();
    }
}

/// Whether the job running is one `same` finds, and goes on (isn't stopped).
fn running_on<J>(running: &Option<(J, Arc<AtomicBool>)>, same: impl Fn(&J) -> bool) -> bool {
    running.as_ref().is_some_and(|(j, stop)| !stop.load(Ordering::Relaxed) && same(j))
}

#[cfg(test)]
mod tests;
