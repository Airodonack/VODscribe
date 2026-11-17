# The app root's tests

- `start.rs`: how the app starts and quits: what a start tidies, the queues taking up what was left, a library it can't read in full, and what a quit leaves to the next start.
- `resume.rs`: unfinished work carries on after a quit in the order it was asked: clips and edits, moved or put first (two drags kept in order), kept videos, erasures, transcriptions resumed or retried, received files, an edit's footage (not once its fetch failed); a library from before the order kept, and jobs an older version asked.
- `addon.rs`: the GPU add-on's download as a job: listed with Cancel, carried on after a quit from the bytes it had, a failure or a Cancel not carried on.
