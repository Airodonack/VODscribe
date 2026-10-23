//! Work done one job at a time, in the order asked, on a thread of its own:
//! clips cut (clips.rs), edits rendered (edits.rs), videos downloaded
//! (videos.rs), storyboards made (storyboards.rs). A queue keeps what's
//! waiting and what's running with its stop, so a job can be taken out
//! before its turn, or stopped during it.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use anyhow::Result;

struct Queue<J> {
    waiting: VecDeque<J>,
    running: Option<(J, Arc<AtomicBool>)>,
}

pub(crate) struct Jobs<J> {
    shared: Arc<(Mutex<Queue<J>>, Condvar)>,
    started: AtomicBool,
}

impl<J: Clone + Send + 'static> Jobs<J> {
    pub(crate) fn new() -> Self {
        let queue = Queue { waiting: VecDeque::new(), running: None };
        Jobs { shared: Arc::new((Mutex::new(queue), Condvar::new())), started: AtomicBool::new(false) }
    }

    /// Adds `job` last.
    pub(crate) fn push(&self, job: J) {
        let (queue, wake) = &*self.shared;
        queue.lock().unwrap().waiting.push_back(job);
        wake.notify_one();
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
            return false;
        }
        if queue.running.as_ref().is_some_and(|(j, _)| same(j)) {
            return false;
        }
        queue.waiting.push_front(job);
        wake.notify_one();
        true
    }

    /// Takes the jobs `which` finds out of those waiting, and returns them.
    pub(crate) fn take_waiting(&self, which: impl Fn(&J) -> bool) -> Vec<J> {
        let mut queue = self.shared.0.lock().unwrap();
        let (taken, kept) = queue.waiting.drain(..).partition(|j| which(j));
        queue.waiting = kept;
        taken.into()
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
                work(job, &stop);
                queue.lock().unwrap().running = None;
                // For `stop_and_wait`.
                wake.notify_all();
            }
        })?;
        Ok(())
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
