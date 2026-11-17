//! The specs of the job queue (`Jobs`): one job at a time, in the order
//! asked, with what's waiting and what's running open to being taken out,
//! stopped or moved, and word of each change. Its jobs are held at a gate
//! where a test needs one running: `held` starts a queue whose jobs wait at
//! the gate till it's opened.

use super::*;

use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use vodscribe_core::testkit::{Gate, eventually};

/// A queue whose jobs each wait at `gate` before they end, noting the order
/// they end in and how many were being made at once at most.
struct Held {
    jobs: Jobs<i32>,
    gate: Gate,
    ended: Arc<Mutex<Vec<(i32, bool)>>>,
    most_at_once: Arc<AtomicUsize>,
}

fn held() -> Held {
    let (jobs, gate) = (Jobs::new(), Gate::new());
    let ended: Arc<Mutex<Vec<(i32, bool)>>> = Arc::default();
    let (active, most_at_once) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (note, at_once, waits) = (ended.clone(), most_at_once.clone(), gate.clone());
    jobs.run("jobs-test", move |n: i32, stop| {
        at_once.fetch_max(active.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
        waits.wait();
        note.lock().unwrap().push((n, stop.load(Ordering::SeqCst)));
        active.fetch_sub(1, Ordering::SeqCst);
    })
    .unwrap();
    Held { jobs, gate, ended, most_at_once }
}

impl Held {
    /// Pushes job `n` and waits till it's the one running, at the gate.
    fn running(&self, n: i32) {
        self.jobs.push(n);
        self.gate.reached();
        assert_eq!(self.jobs.snapshot().0, Some(n));
    }

    /// Lets the jobs go and waits till `n` have ended: what they ended as.
    fn done(&self, n: usize) -> Vec<(i32, bool)> {
        self.gate.open();
        eventually("the jobs ended", || self.ended.lock().unwrap().len() >= n);
        self.ended.lock().unwrap().clone()
    }
}

/// Jobs run one at a time, in the order they were added (module doc: "Work
/// done one job at a time, in the order asked").
#[test]
fn jobs_run_one_at_a_time_in_the_order_asked() {
    let queue = held();
    queue.running(1);
    for n in [2, 3] {
        queue.jobs.push(n);
    }
    assert_eq!(queue.jobs.snapshot(), (Some(1), vec![2, 3]), "the first is made, the others wait");
    let ended: Vec<i32> = queue.done(3).into_iter().map(|(n, _)| n).collect();
    assert_eq!(ended, [1, 2, 3]);
    assert_eq!(queue.most_at_once.load(Ordering::SeqCst), 1);
}

/// A job waiting can be taken out before its turn (module doc: "a job can be
/// taken out before its turn"; `take_waiting`: "Takes the jobs `which` finds
/// out of those waiting, and returns them").
#[test]
fn a_job_waiting_can_be_taken_out_before_its_turn() {
    let queue = held();
    queue.running(1);
    for n in [2, 3, 4] {
        queue.jobs.push(n);
    }
    assert_eq!(queue.jobs.take_waiting(|&j| j == 3), [3]);
    assert_eq!(queue.jobs.take_waiting(|&j| j == 1), Vec::<i32>::new(), "the one running isn't waiting");
    let ended: Vec<i32> = queue.done(3).into_iter().map(|(n, _)| n).collect();
    assert_eq!(ended, [1, 2, 4], "3 never ran");
}

/// The job running can be stopped during its turn, and only when it's one
/// the caller names (module doc: "stopped during it"; `stop_running`: "Stops
/// the job running if it's one `which` finds; whether it was").
#[test]
fn the_job_running_can_be_stopped_during_its_turn() {
    let queue = held();
    queue.running(1);
    queue.jobs.push(2);
    assert!(!queue.jobs.stop_running(|&j| j == 2), "2 isn't running");
    assert!(queue.jobs.stop_running(|&j| j == 1));
    assert_eq!(queue.done(2), [(1, true), (2, false)], "only the one stopped is told to stop");
}

/// A job asked for while it waits waits once (`push_once`: "Adds `job` last
/// unless one `same` finds is waiting already; whether it did").
#[test]
fn a_job_asked_for_again_while_it_waits_waits_once() {
    let queue = held();
    queue.running(1);
    assert!(queue.jobs.push_once(2, |&j| j == 2));
    assert!(!queue.jobs.push_once(2, |&j| j == 2), "asked again while it waits: once is enough");
    assert_eq!(queue.jobs.snapshot(), (Some(1), vec![2]));
    queue.done(2);
}

/// A job asked for again while it runs waits to run again after it
/// (`push_once`: "Adds `job` last unless one `same` finds is waiting
/// already"; as renders are asked for: once while waiting, again once
/// running).
#[test]
fn a_job_asked_for_while_it_runs_is_queued_to_run_again_after() {
    let queue = held();
    queue.running(1);
    assert!(queue.jobs.push_once(1, |&j| j == 1));
    assert_eq!(queue.jobs.snapshot(), (Some(1), vec![1]));
    assert_eq!(queue.done(2), [(1, false), (1, false)]);
}

/// A job isn't added by `push_new` while one the same is waiting, or is
/// running and not stopped (`push_new`: "Adds `job` last unless one `same`
/// finds is waiting or running (and not stopped); whether it did").
#[test]
fn a_new_job_is_not_added_while_one_the_same_waits_or_runs() {
    let queue = held();
    queue.running(1);
    assert!(!queue.jobs.push_new(1, |&j| j == 1), "running: it's the same job");
    assert!(queue.jobs.push_new(2, |&j| j == 2));
    assert!(!queue.jobs.push_new(2, |&j| j == 2), "waiting: it's the same job");
    assert_eq!(queue.jobs.snapshot(), (Some(1), vec![2]));
    queue.done(2);
}

/// A clip deleted while its cut winds down, and another made at once that
/// SQLite gives the same id: the new one is queued, not taken for the one
/// ending (it waited, never made: drag-promise on macOS, 2026-11-01;
/// `push_front`: "one being stopped is ending, and a job asked for now is a
/// new one").
#[test]
fn a_job_asked_for_while_one_of_its_id_is_being_stopped_is_queued() {
    let queue = held();
    queue.running(7);
    assert!(!queue.jobs.push_front(7, |&j| j == 7), "running and going on: the same job");
    assert!(!queue.jobs.push_new(7, |&j| j == 7), "running and going on: the same job");
    assert!(queue.jobs.stop_running(|&j| j == 7));
    assert!(queue.jobs.push_front(7, |&j| j == 7), "being stopped: a new one");
    assert!(!queue.jobs.push_new(7, |&j| j == 7), "waiting now");
    assert_eq!(queue.jobs.snapshot(), (Some(7), vec![7]));
    queue.done(2);
}

/// A job pushed to the front goes next: moved there if it's waiting, added
/// there unless one is running (`push_front`: "Puts `job` first: the one
/// `same` finds waiting is moved there, or `job` is added there unless one is
/// running").
#[test]
fn a_job_pushed_to_the_front_goes_next() {
    let queue = held();
    queue.running(0);
    for n in [1, 2, 3] {
        queue.jobs.push(n);
    }
    assert!(!queue.jobs.push_front(3, |&j| j == 3), "moved, not added");
    assert!(queue.jobs.push_front(4, |&j| j == 4));
    assert_eq!(queue.jobs.listed(), [0, 4, 3, 1, 2]);
    assert!(!queue.jobs.push_front(0, |&j| j == 0), "running already");
    assert_eq!(queue.jobs.listed(), [0, 4, 3, 1, 2]);
    queue.done(5);
}

/// The jobs waiting that an order names are put in its order, in the places
/// they hold; the others keep theirs, and the one running stays (`reorder`;
/// module doc: "put in another order"; as the library's list of jobs is
/// dragged into another order).
#[test]
fn jobs_waiting_are_put_in_the_order_asked_and_others_keep_their_places() {
    let queue = held();
    queue.running(9);
    for n in [1, 2, 3, 4, 5] {
        queue.jobs.push(n);
    }
    // 4 dragged above 2; 3 and 5 came after the order was read.
    queue.jobs.reorder(&[1, 4, 2, 9], |&j| j);
    assert_eq!(queue.jobs.listed(), [9, 1, 4, 3, 2, 5], "2 and 4 swap places; 3 and 5 stay; 9 is running");
    queue.jobs.reorder(&[5, 1], |&j| j);
    assert_eq!(queue.jobs.listed(), [9, 5, 4, 3, 2, 1]);
    queue.jobs.reorder(&[7, 8], |&j| j);
    assert_eq!(queue.jobs.listed(), [9, 5, 4, 3, 2, 1], "none of them waiting: nothing moves");
    queue.done(6);
}

/// The job running, then those waiting, are listed in order (`listed`,
/// `snapshot`).
#[test]
fn the_job_running_and_those_waiting_are_listed_in_order() {
    let queue = held();
    assert_eq!((queue.jobs.snapshot(), queue.jobs.listed()), ((None, vec![]), vec![]), "nothing asked");
    queue.running(1);
    queue.jobs.push(2);
    queue.jobs.push(3);
    assert_eq!(queue.jobs.snapshot(), (Some(1), vec![2, 3]));
    assert_eq!(queue.jobs.listed(), [1, 2, 3]);
    queue.done(3);
}

/// A queue tells each job added, taken out or moved, and no change it didn't
/// make (`watch`: "Calls `changed` after each change to what's waiting or
/// running: a job added, taken out, begun, ended or moved"), for the list of
/// jobs the library shows.
#[test]
fn each_job_added_taken_out_or_moved_is_told_and_no_change_is_not() {
    let jobs = Jobs::new();
    let told = Arc::new(AtomicUsize::new(0));
    let count = told.clone();
    jobs.watch(move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let seen = || told.load(Ordering::SeqCst);
    jobs.push(1);
    assert_eq!(seen(), 1);
    assert!(jobs.push_once(2, |&j| j == 2) && !jobs.push_once(2, |&j| j == 2));
    assert_eq!(seen(), 2, "not added again: no change");
    assert!(jobs.push_new(3, |&j| j == 3));
    assert!(!jobs.push_new(3, |&j| j == 3));
    assert_eq!(seen(), 3, "not added again: no change");
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
}

/// A queue tells each job begun and each ended (`watch`: "a job added, taken
/// out, begun, ended or moved").
#[test]
fn each_job_begun_and_ended_is_told() {
    let jobs = Jobs::new();
    let told = Arc::new(AtomicUsize::new(0));
    let count = told.clone();
    jobs.watch(move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let gate = Gate::new();
    let waits = gate.clone();
    jobs.run("watch-test", move |_: i32, _| waits.wait()).unwrap();
    jobs.push(1);
    gate.reached();
    eventually("told it was added and begun", || told.load(Ordering::SeqCst) == 2);
    gate.open();
    eventually("told it ended", || told.load(Ordering::SeqCst) == 3);
}

/// As Cancel in the list of jobs lands as a cut ends (`Cutter::cancel_job`):
/// the job has done its work but is still the one running. The mark
/// "canceled while running" is made and taken under the queue's lock, so it
/// never outlives the job, to show a later run of the same id as canceled
/// (`run_ended`, `stop_running_and_mark`).
#[test]
fn a_job_canceled_as_it_ends_leaves_no_mark() {
    let jobs = Jobs::new();
    let marks: Arc<Mutex<std::collections::HashSet<i32>>> = Arc::default();
    let opened = Gate::new();
    let (settled, was_settled) = std::sync::mpsc::channel();
    let taken = marks.clone();
    let held = opened.clone();
    jobs.run_ended(
        "cancel-test",
        move |_: i32, _| {
            // Its work done, about to return: the cancel comes here.
            held.wait();
        },
        move |&n| taken.lock().unwrap().remove(&n),
        move |n, marked| settled.send((n, marked)).unwrap(),
    )
    .unwrap();
    jobs.push(1);
    opened.reached();
    let mark = marks.clone();
    let canceled = jobs.stop_running_and_mark(
        |&j| j == 1,
        |&j| {
            mark.lock().unwrap().insert(j);
        },
    );
    assert!(canceled, "still running");
    opened.open();
    assert_eq!(was_settled.recv_timeout(Duration::from_secs(5)), Ok((1, true)), "its end found the mark");
    assert!(marks.lock().unwrap().is_empty(), "a mark left behind: {:?}", marks.lock().unwrap());
    assert!(!jobs.stop_running_and_mark(|&j| j == 1, |_| panic!("not running")));
}

/// A job is wanted while it waits, or runs and isn't stopped: one that will
/// still write what it makes (`is_wanted`; `is_waiting`).
#[test]
fn a_job_is_wanted_while_waiting_or_running_unstopped() {
    let queue = held();
    queue.running(1);
    queue.jobs.push(2);
    assert!(queue.jobs.is_wanted(|&j| j == 2) && queue.jobs.is_waiting(|&j| j == 2));
    assert!(queue.jobs.is_wanted(|&j| j == 1) && !queue.jobs.is_waiting(|&j| j == 1), "running, not waiting");
    assert!(!queue.jobs.is_wanted(|&j| j == 3));
    queue.jobs.stop_running(|&j| j == 1);
    assert!(!queue.jobs.is_wanted(|&j| j == 1), "stopped: it deletes what it made");
    queue.done(2);
}

/// A job stopped and waited for has ended when the wait is over, and one
/// that is another isn't waited for (`stop_and_wait`; as a storyboard's job
/// lets go of a file about to be deleted).
#[test]
fn a_job_stopped_and_waited_for_has_ended() {
    let queue = held();
    queue.running(1);
    assert!(queue.jobs.stop_and_wait(|&j| j == 2, Duration::ZERO), "another one: nothing to wait for");
    std::thread::scope(|scope| {
        let waiting = scope.spawn(|| queue.jobs.stop_and_wait(|&j| j == 1, Duration::from_secs(5)));
        eventually("stopped", || !queue.jobs.is_wanted(|&j| j == 1));
        assert!(queue.ended.lock().unwrap().is_empty(), "ended before it was let go");
        queue.gate.open();
        assert!(waiting.join().unwrap(), "returned with the job still running");
    });
    assert_eq!(*queue.ended.lock().unwrap(), [(1, true)], "returned before the job ended");
}

/// The wait for a job to end gives up after its time, and says the job is
/// still running (`stop_and_wait`: "waits up to `timeout` for it to end;
/// whether none is running now").
#[test]
fn the_wait_for_a_job_to_end_gives_up_and_says_it_is_still_running() {
    let queue = held();
    queue.running(1);
    assert!(!queue.jobs.stop_and_wait(|&j| j == 1, Duration::ZERO));
    assert!(!queue.jobs.is_wanted(|&j| j == 1), "it was told to stop");
    queue.done(1);
}
