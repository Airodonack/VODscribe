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
}

/// What's told of each change to a queue (`Jobs::watch`).
type Watcher = Arc<OnceLock<Box<dyn Fn() + Send + Sync>>>;

pub(crate) struct Jobs<J> {
    shared: Arc<(Mutex<Queue<J>>, Condvar)>,
    started: AtomicBool,
    watcher: Watcher,
}

impl<J: Clone + Send + 'static> Jobs<J> {
    pub(crate) fn new() -> Self {
        let queue = Queue { waiting: VecDeque::new(), running: None };
        Jobs {
            shared: Arc::new((Mutex::new(queue), Condvar::new())),
            started: AtomicBool::new(false),
            watcher: Arc::default(),
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

    /// Adds `job` last unless one `same` finds is waiting or running; whether it did.
    pub(crate) fn push_new(&self, job: J, same: impl Fn(&J) -> bool) -> bool {
        let (queue, wake) = &*self.shared;
        let mut queue = queue.lock().unwrap();
        if queue.running.as_ref().is_some_and(|(j, _)| same(j)) || queue.waiting.iter().any(same) {
            return false;
        }
        queue.waiting.push_back(job);
        wake.notify_one();
        drop(queue);
        self.changed();
        true
    }

    /// Puts `job` first: the one `same` finds waiting is moved there, or
    /// `job` is added there unless one is running; whether it was added.
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
        if queue.running.as_ref().is_some_and(|(j, _)| same(j)) {
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
        // A second thread would run two jobs at once, and `running` holds one.
        let again = self.started.swap(true, Ordering::Relaxed);
        debug_assert!(!again, "the {name} queue's thread was started twice");
        if again {
            return Ok(());
        }
        let shared = self.shared.clone();
        let watcher = self.watcher.clone();
        std::thread::Builder::new().name(name.into()).spawn(move || {
            let (queue, wake) = &*shared;
            loop {
                let (job, stop) = {
                    let mut queue = queue.lock().unwrap();
                    let job = loop {
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
                work(job, &stop);
                queue.lock().unwrap().running = None;
                // For `stop_and_wait`.
                wake.notify_all();
                tell(&watcher);
            }
        })?;
        Ok(())
    }
}

fn tell(watcher: &Watcher) {
    if let Some(changed) = watcher.get() {
        changed();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn jobs_run_in_order_one_at_a_time_and_can_be_taken_out_or_stopped() {
        let jobs = Jobs::new();
        let (done, finished) = mpsc::channel();
        let (started, began) = mpsc::channel();
        jobs.run("test", move |n: i32, stop| {
            started.send(n).unwrap();
            // The first runs till it's stopped.
            while n == 1 && !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(5));
            }
            done.send((n, stop.load(Ordering::Relaxed))).unwrap();
        })
        .unwrap();
        jobs.push(1);
        assert_eq!(began.recv_timeout(Duration::from_secs(5)), Ok(1));
        for n in [2, 3, 4] {
            jobs.push(n);
        }
        assert!(!jobs.push_once(3, |&j| j == 3), "3 is waiting already");
        assert_eq!(jobs.listed(), [1, 2, 3, 4]);
        assert_eq!(jobs.take_waiting(|&j| j == 3), [3]);
        assert!(!jobs.stop_running(|&j| j == 2), "2 isn't running");
        assert!(jobs.stop_running(|&j| j == 1));
        let all: Vec<_> = (0..3).map(|_| finished.recv_timeout(Duration::from_secs(5)).unwrap()).collect();
        assert_eq!(all, [(1, true), (2, false), (4, false)], "3 never ran");
    }

    /// As renders are asked for: once while waiting, again once running.
    #[test]
    fn a_job_asked_for_again_waits_once() {
        let jobs = Jobs::new();
        assert!(jobs.push_once(1, |&j| j == 1) && jobs.push_once(2, |&j| j == 2));
        assert!(!jobs.push_once(1, |&j| j == 1), "asked again while it waits: once is enough");
        assert!(!jobs.push_new(2, |&j| j == 2));
        // 1 begins (taken, as the thread does), then is asked for again.
        jobs.shared.0.lock().unwrap().running = Some((1, Arc::default()));
        jobs.take_waiting(|&j| j == 1);
        assert!(!jobs.push_new(1, |&j| j == 1), "running: push_new won't");
        assert!(jobs.push_once(1, |&j| j == 1), "running: push_once will, to run again after");
        assert_eq!(jobs.listed(), [1, 2, 1]);
    }

    /// As a storyboard's job lets go of a file about to be deleted.
    #[test]
    fn a_job_stopped_and_waited_for_has_ended() {
        let jobs = Jobs::new();
        let (began, has_begun) = mpsc::channel();
        let ended = Arc::new(AtomicBool::new(false));
        let mark = ended.clone();
        jobs.run("jobs-test", move |_: i32, stop| {
            began.send(()).unwrap();
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(10));
            }
            std::thread::sleep(Duration::from_millis(100));
            mark.store(true, Ordering::Relaxed);
        })
        .unwrap();
        jobs.push(1);
        has_begun.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(jobs.stop_and_wait(|&j| j == 2, Duration::ZERO), "another one: nothing to wait for");
        assert!(jobs.stop_and_wait(|&j| j == 1, Duration::from_secs(5)));
        assert!(ended.load(Ordering::Relaxed), "returned before the job ended");
    }

    /// As a storyboard is for the VOD opened: first, moved there if it's waiting.
    #[test]
    fn a_job_pushed_to_the_front_goes_next() {
        let jobs = Jobs::new();
        for n in [1, 2, 3] {
            jobs.push(n);
        }
        assert!(!jobs.push_front(3, |&j| j == 3), "moved, not added");
        assert!(jobs.push_front(4, |&j| j == 4));
        assert_eq!(jobs.listed(), [4, 3, 1, 2]);
        jobs.shared.0.lock().unwrap().running = Some((5, Arc::default()));
        assert!(!jobs.push_front(5, |&j| j == 5), "running already");
        assert_eq!(jobs.listed(), [5, 4, 3, 1, 2]);
    }

    /// As the library's list of jobs is dragged into another order.
    #[test]
    fn jobs_waiting_are_put_in_the_order_asked_and_others_keep_their_places() {
        let jobs = Jobs::new();
        for n in [1, 2, 3, 4, 5] {
            jobs.push(n);
        }
        jobs.shared.0.lock().unwrap().running = Some((9, Arc::default()));
        // 4 dragged above 2; 3 and 5 came after the order was read.
        jobs.reorder(&[1, 4, 2, 9], |&j| j);
        assert_eq!(jobs.listed(), [9, 1, 4, 3, 2, 5], "2 and 4 swap places; 3 and 5 stay; 9 is running");
        jobs.reorder(&[5, 1], |&j| j);
        assert_eq!(jobs.listed(), [9, 5, 4, 3, 2, 1]);
        jobs.reorder(&[7, 8], |&j| j);
        assert_eq!(jobs.listed(), [9, 5, 4, 3, 2, 1], "none of them waiting: nothing moves");
    }

    /// For the library's list of jobs: word of each change, none for no change.
    #[test]
    fn each_change_to_a_queue_is_told() {
        let jobs = Jobs::new();
        let told = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = told.clone();
        jobs.watch(move || {
            count.fetch_add(1, Ordering::Relaxed);
        });
        let seen = || told.load(Ordering::Relaxed);
        jobs.push(1);
        assert_eq!(seen(), 1);
        assert!(jobs.push_once(2, |&j| j == 2) && !jobs.push_once(2, |&j| j == 2));
        assert_eq!(seen(), 2, "not added again: no change");
        assert!(jobs.push_new(3, |&j| j == 3));
        assert!(!jobs.push_front(3, |&j| j == 3), "moved to the front");
        assert_eq!(seen(), 4);
        jobs.reorder(&[3, 1, 2], |&j| j);
        assert_eq!(seen(), 4, "in that order already");
        jobs.reorder(&[2, 1], |&j| j);
        assert_eq!(seen(), 5);
        assert!(jobs.take_waiting(|&j| j == 7).is_empty());
        assert_eq!(seen(), 5, "nothing taken");
        assert_eq!(jobs.take_waiting(|&j| j == 1), [1]);
        assert_eq!(seen(), 6);
        // Begun and ended, on the queue's thread.
        let (done, finished) = mpsc::channel();
        jobs.run("watch-test", move |n: i32, _| done.send(n).unwrap()).unwrap();
        let ran: Vec<_> = (0..2).map(|_| finished.recv_timeout(Duration::from_secs(5)).unwrap()).collect();
        assert_eq!(ran, [3, 2]);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while seen() < 10 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(seen(), 10, "each begun and ended");
    }

    #[test]
    fn a_job_is_wanted_while_waiting_or_running_unstopped() {
        let jobs = Jobs::new();
        jobs.push(2);
        assert!(jobs.is_wanted(|&j| j == 2) && !jobs.is_wanted(|&j| j == 1));
        jobs.shared.0.lock().unwrap().running = Some((1, Arc::default()));
        assert!(jobs.is_wanted(|&j| j == 1));
        jobs.stop_running(|&j| j == 1);
        assert!(!jobs.is_wanted(|&j| j == 1), "stopped: it deletes what it made");
    }
}
