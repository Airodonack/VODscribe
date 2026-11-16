//! A drop played without a system's drag, for the tests of an app that
//! takes one: virtual files made up by name and size ([`item`]), and a fetch
//! of those the app takes ([`fetch`]) whose bytes the test gives, through
//! the same destinations, progress and cancel a system's fetch has. Each
//! item ends once: whole, failed as a source that doesn't give it, or, let
//! go of unfinished, canceled, as a fetch dropped early.

use std::sync::Arc;

use crate::{Destination, Error, Fetch, Item, Job, Shared};

/// A virtual file named `name`, `size` bytes long if its source says.
pub fn item(name: &str, size: Option<u64>) -> Item {
    Item { name: name.into(), size, index: 0 }
}

/// Fetches `requests` as a system's fetch would: each item's bytes are
/// what the test gives its [`Giving`], in the order asked.
pub fn fetch(requests: Vec<(Item, Destination)>) -> Vec<Giving> {
    let (shared, jobs) = Shared::new(requests);
    let fetch = Arc::new(shared.handle(true));
    jobs.into_iter().map(|job| Giving { shared: shared.clone(), job: Some(job), fetch: fetch.clone() }).collect()
}

/// One item of a fake fetch, its bytes given by the test.
pub struct Giving {
    shared: Arc<Shared>,
    /// Till it's ended.
    job: Option<Job>,
    /// The fetch, kept while any of its items is: dropped, it cancels.
    fetch: Arc<Fetch>,
}

impl Giving {
    /// Its name, as its source gave it.
    pub fn name(&self) -> &str {
        self.job.as_ref().map_or("", |job| job.item.name())
    }

    /// The fetch it's part of: its progress, its cancel.
    pub fn fetch(&self) -> &Fetch {
        &self.fetch
    }

    /// Its next `bytes`. Refused (the fetch canceled, the destination's
    /// write failed), it ends with why, as a system's fetch ends it.
    pub fn give(&mut self, bytes: &[u8]) -> Result<(), String> {
        let Some(job) = self.job.as_mut() else { return Err("it has ended".into()) };
        match self.shared.write(job, bytes) {
            Ok(()) => Ok(()),
            Err(e) => {
                let why = e.to_string();
                self.end(Err(e));
                Err(why)
            }
        }
    }

    /// It's whole: what was given is all of it.
    pub fn whole(mut self) {
        let got = self.fetch.progress().get(self.slot()).map_or(0, |p| p.got);
        self.end(Ok(got));
    }

    /// Its source doesn't give the rest, saying `why`.
    pub fn fails(mut self, why: &str) {
        self.end(Err(Error::Source(why.into())));
    }

    fn slot(&self) -> usize {
        self.job.as_ref().map_or(0, |job| job.slot)
    }

    fn end(&mut self, result: Result<u64, Error>) {
        if let Some(job) = self.job.take() {
            self.shared.end(job, result);
        }
    }
}

impl Drop for Giving {
    /// Let go of unfinished: canceled, as a fetch dropped early ends it.
    fn drop(&mut self) {
        self.end(Err(Error::Canceled));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{Kept, fresh, names};
    use std::sync::mpsc;

    /// Module doc: "through the same destinations, progress and cancel a system's fetch has": into a
    /// folder, an item given whole is there under its name.
    #[test]
    fn an_item_given_whole_into_a_folder_is_there_under_its_name() {
        let dir = fresh("fake-whole");
        let (tell, heard) = mpsc::channel();
        let mut a = fetch(vec![(item("a.mp4", Some(4)), Destination::folder(&dir, move |r| tell.send(r).unwrap()))]);
        let mut a = a.remove(0);
        assert_eq!(a.name(), "a.mp4");
        a.give(b"ab").unwrap();
        assert_eq!(a.fetch().progress()[0].got, 2);
        a.give(b"cd").unwrap();
        a.whole();
        assert_eq!(heard.recv().unwrap().unwrap(), dir.join("a.mp4"));
        assert_eq!(std::fs::read(dir.join("a.mp4")).unwrap(), b"abcd");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Module doc: "failed as a source that doesn't give it": the folder keeps nothing and its
    /// callback hears why.
    #[test]
    fn an_item_its_source_fails_leaves_nothing_and_says_so() {
        let dir = fresh("fake-fails");
        let (tell, heard) = mpsc::channel();
        let mut b = fetch(vec![(item("b.mp4", None), Destination::folder(&dir, move |r| tell.send(r).unwrap()))]);
        let mut b = b.remove(0);
        b.give(b"x").unwrap();
        b.fails("refused");
        assert_eq!(heard.recv().unwrap().unwrap_err().to_string(), "the app it came from didn't give it: refused");
        assert_eq!(names(&dir), Vec::<String>::new());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Module doc: "let go of unfinished, canceled, as a fetch dropped early".
    #[test]
    fn an_item_let_go_of_unfinished_ends_canceled_and_leaves_nothing() {
        let dir = fresh("fake-let-go");
        let (tell, heard) = mpsc::channel();
        let mut c = fetch(vec![(item("c.mp4", None), Destination::folder(&dir, move |r| tell.send(r).unwrap()))]);
        let mut c = c.remove(0);
        c.give(b"x").unwrap();
        drop(c);
        assert!(matches!(heard.recv().unwrap(), Err(Error::Canceled)));
        assert_eq!(names(&dir), Vec::<String>::new());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `Giving::give`: "Refused (the fetch canceled ...), it ends with why, as a system's fetch ends
    /// it."
    #[test]
    fn a_fake_fetch_canceled_refuses_the_next_bytes_and_ends_its_items() {
        let kept = Kept::default();
        let mut items = fetch(vec![(item("a.mp4", None), Destination::sink(kept.clone()))]);
        let mut a = items.remove(0);
        a.give(b"ab").unwrap();
        a.fetch().cancel();
        assert_eq!(a.give(b"cd"), Err("canceled".to_string()));
        assert!(a.fetch().is_finished());
        assert_eq!(kept.given(), (b"ab".to_vec(), Some(Err("canceled".into()))));
    }

    /// `Giving::give`: "(the destination's write failed), it ends with why".
    #[test]
    fn a_write_the_destination_refuses_ends_the_item_with_why() {
        let dir = fresh("fake-refused");
        let blocked = dir.join("not-a-dir");
        std::fs::write(&blocked, b"").unwrap();
        let (tell, heard) = mpsc::channel();
        let mut items = fetch(vec![(
            item("a.mp4", None),
            Destination::folder(blocked.join("under"), move |r| tell.send(r).unwrap()),
        )]);
        let mut a = items.remove(0);
        assert!(a.give(b"x").unwrap_err().starts_with("it couldn't be written"));
        assert!(matches!(heard.recv().unwrap(), Err(Error::Write(_))));
        assert_eq!(a.give(b"y"), Err("it has ended".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Module doc: "Each item ends once": each of several items of one fake fetch ends by itself.
    #[test]
    fn each_item_of_a_fake_fetch_ends_on_its_own() {
        let kept = (Kept::default(), Kept::default());
        let mut items = fetch(vec![
            (item("a.mp4", None), Destination::sink(kept.0.clone())),
            (item("b.mp4", None), Destination::sink(kept.1.clone())),
        ])
        .into_iter();
        let (mut a, mut b) = (items.next().unwrap(), items.next().unwrap());
        a.give(b"a").unwrap();
        a.whole();
        assert!(kept.1.given().1.is_none());
        b.give(b"bbb").unwrap();
        b.whole();
        assert_eq!(kept.0.given(), (b"a".to_vec(), Some(Ok(1))));
        assert_eq!(kept.1.given(), (b"bbb".to_vec(), Some(Ok(3))));
    }

    /// `Giving::fails`: "Its source doesn't give the rest, saying `why`": the other items go on.
    #[test]
    fn an_item_its_source_fails_leaves_the_others_to_go_on() {
        let kept = (Kept::default(), Kept::default());
        let mut items = fetch(vec![
            (item("a.mp4", None), Destination::sink(kept.0.clone())),
            (item("b.mp4", None), Destination::sink(kept.1.clone())),
        ])
        .into_iter();
        let (a, mut b) = (items.next().unwrap(), items.next().unwrap());
        a.fails("no");
        assert!(kept.1.given().1.is_none());
        b.give(b"b").unwrap();
        b.whole();
        assert_eq!(kept.0.given().1, Some(Err("the app it came from didn't give it: no".into())));
        assert_eq!(kept.1.given(), (b"b".to_vec(), Some(Ok(1))));
    }
}
