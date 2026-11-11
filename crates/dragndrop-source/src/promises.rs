//! A drag's file promises (macOS): how many were asked for, are being
//! written, and were written. The three are counted under one lock, so the
//! drag's end reads them as they stood at one moment: read one by one, a
//! promise asked for between the reads was counted asked for and not
//! written, and the drag was said not to be delivered while Finder got the
//! file. Platform-free, so its tests run everywhere.

use std::sync::Mutex;

/// A drag's promises.
#[derive(Default)]
pub(crate) struct Promises(Mutex<Counts>);

/// How many promises were asked for, are being written, and were written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub asked: usize,
    pub writing: usize,
    pub written: usize,
}

impl Promises {
    /// One asked for, being written from now.
    pub(crate) fn asked(&self) {
        let mut counts = self.0.lock().unwrap();
        counts.asked += 1;
        counts.writing += 1;
    }

    /// One being written is done: written, or not (it failed, or the
    /// receiver called it off).
    pub(crate) fn done(&self, written: bool) {
        let mut counts = self.0.lock().unwrap();
        counts.writing -= 1;
        if written {
            counts.written += 1;
        }
    }

    /// How they stand once none is being written; None while one is (its
    /// end settles the drag).
    pub(crate) fn settled(&self) -> Option<Counts> {
        let counts = *self.0.lock().unwrap();
        (counts.writing == 0).then_some(counts)
    }
}

#[cfg(test)]
mod tests;
