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
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// A fresh folder for a test.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dragndrop-fake-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Into a folder: whole, it's there; failed or let go of, nothing is.
    #[test]
    fn a_fake_fetch_writes_what_its_test_gives() {
        let dir = folder("folder");
        let ended: Arc<Mutex<Vec<String>>> = Arc::default();
        let to = |ended: &Arc<Mutex<Vec<String>>>| {
            let ended = ended.clone();
            Destination::folder(&dir, move |r| {
                ended.lock().unwrap().push(r.map(|p| p.display().to_string()).unwrap_or_else(|e| e.to_string()))
            })
        };
        let mut items = fetch(vec![
            (item("a.mp4", Some(4)), to(&ended)),
            (item("b.mp4", None), to(&ended)),
            (item("c.mp4", None), to(&ended)),
        ])
        .into_iter();
        let (mut a, mut b, c) = (items.next().unwrap(), items.next().unwrap(), items.next().unwrap());
        a.give(b"ab").unwrap();
        assert_eq!(a.fetch().progress()[0].got, 2);
        a.give(b"cd").unwrap();
        a.whole();
        assert_eq!(std::fs::read(dir.join("a.mp4")).unwrap(), b"abcd");
        b.give(b"x").unwrap();
        b.fails("refused");
        drop(c);
        assert_eq!(
            *ended.lock().unwrap(),
            [
                dir.join("a.mp4").display().to_string(),
                "the app it came from didn't give it: refused".to_string(),
                "canceled".to_string()
            ]
        );
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, ["a.mp4"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Canceled, the next bytes are refused and the item ends so.
    #[test]
    fn a_fake_fetch_canceled_refuses_the_next_bytes() {
        let dir = folder("cancel");
        let mut items = fetch(vec![(item("a.mp4", None), Destination::folder(&dir, |_| {}))]);
        let mut a = items.remove(0);
        a.give(b"ab").unwrap();
        a.fetch().cancel();
        assert_eq!(a.give(b"cd"), Err("canceled".to_string()));
        assert!(a.fetch().is_finished());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
