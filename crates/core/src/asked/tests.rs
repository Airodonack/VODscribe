//! The specs of `asked.rs`: the order unfinished work carries on in.

use super::*;

fn library() -> Db {
    Db::open(std::path::Path::new(":memory:")).unwrap()
}

/// Jobs come back in the order they were asked, whatever their ids
/// (`Db::in_asked_order`: "in the order they were asked").
#[test]
fn jobs_come_back_in_the_order_asked() {
    let db = library();
    for job in [3, 1, 2] {
        db.ask(Queue::Render, job).unwrap();
    }
    assert_eq!(db.in_asked_order(Queue::Render, vec![1, 2, 3]).unwrap(), [3, 1, 2]);
}

/// One counter across every queue (db.rs `MIGRATE_V47`: "so jobs of two
/// queues compare too"): a job asked later in another queue is later.
#[test]
fn jobs_of_two_queues_compare_on_one_counter() {
    let db = library();
    db.ask(Queue::Video, 7).unwrap();
    db.ask(Queue::Transcribe, 1).unwrap();
    db.ask(Queue::Video, 8).unwrap();
    let at = |queue, job| {
        db.conn
            .query_row("SELECT at FROM asked WHERE queue = ?1 AND job = ?2", params![queue, job], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap()
    };
    assert!(at("video", 7) < at("transcribe", 1) && at("transcribe", 1) < at("video", 8));
}

/// A job asked again takes the place of one asked now (`Db::ask`): last.
#[test]
fn a_job_asked_again_goes_last() {
    let db = library();
    for job in [1, 2, 3] {
        db.ask(Queue::Render, job).unwrap();
    }
    db.ask(Queue::Render, 1).unwrap();
    assert_eq!(db.in_asked_order(Queue::Render, vec![1, 2, 3]).unwrap(), [2, 3, 1]);
}

/// A job put first (`Db::ask_first`) is ahead of the queue's others,
/// though other queues' jobs were asked before it.
#[test]
fn a_job_put_first_is_ahead_of_its_queues_others() {
    let db = library();
    db.ask(Queue::Erase, 9).unwrap();
    for job in [1, 2] {
        db.ask(Queue::Render, job).unwrap();
    }
    db.ask_first(Queue::Render, 3).unwrap();
    db.ask_first(Queue::Render, 2).unwrap();
    assert_eq!(db.in_asked_order(Queue::Render, vec![1, 2, 3]).unwrap(), [2, 3, 1]);
    // The first of a queue that has none yet is simply asked.
    db.ask_first(Queue::Video, 4).unwrap();
    assert!(db.is_asked(Queue::Video, 4).unwrap());
}

/// Moved in the list of jobs (`Db::reorder_asked`), the jobs moved trade
/// their places; the others keep theirs, as `Jobs::reorder` keeps them.
#[test]
fn jobs_moved_in_the_list_trade_places_and_others_keep_theirs() {
    let db = library();
    for job in [1, 2, 3, 4, 5] {
        db.ask(Queue::Render, job).unwrap();
    }
    // 4 dragged above 2, as jobs.rs's own spec drags it.
    db.reorder_asked(Queue::Render, &[1, 4, 2]).unwrap();
    assert_eq!(db.in_asked_order(Queue::Render, vec![1, 2, 3, 4, 5]).unwrap(), [1, 4, 3, 2, 5]);
}

/// Jobs with no place (from before the table, or asked by an older
/// version) come after those with one, in the order given, and get a
/// place in it; rows of jobs no longer unfinished go
/// (`Db::in_asked_order`).
#[test]
fn jobs_with_no_place_come_after_and_rows_of_others_go() {
    let db = library();
    db.ask(Queue::Render, 5).unwrap();
    db.ask(Queue::Render, 9).unwrap();
    db.ask(Queue::Erase, 5).unwrap();
    assert_eq!(db.in_asked_order(Queue::Render, vec![2, 5, 1, 2]).unwrap(), [5, 2, 1], "each once");
    assert!(!db.is_asked(Queue::Render, 9).unwrap(), "a job no longer unfinished kept its row");
    assert!(db.is_asked(Queue::Erase, 5).unwrap(), "another queue's row went");
    assert_eq!(db.asked(Queue::Render).unwrap(), [5, 2, 1], "places given in that order");
    // More than an id: sorted by its key.
    let sweeps = db.ordered_as_asked(Queue::Render, vec![(1, "b"), (5, "a"), (1, "c")], |j| j.0);
    assert_eq!(sweeps, [(5, "a"), (1, "b")]);
}

/// A queue's start carries its jobs on in the order given when the library
/// can't say the order (`Db::ordered_as_asked`: "out of order … is no
/// reason not to carry them on").
#[test]
fn jobs_come_back_in_the_order_given_when_the_order_cant_be_read() {
    let db = library();
    db.ask(Queue::Erase, 2).unwrap();
    db.conn.execute_batch("DROP TABLE asked").unwrap();
    assert_eq!(db.ordered_as_asked(Queue::Erase, vec![3, 2], |&j| j), [3, 2]);
    db.keep_place(Queue::Erase, 4);
}
