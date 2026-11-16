//! Which of a drag's two offers of its files to take on macOS, the file
//! URLs or the file promises, told from what each pasteboard item says it
//! is, item by item. Kept apart from the AppKit calls so that it's tested everywhere.

/// One pasteboard item of a drag, by the types it offers its file as.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Offered<'a> {
    /// The type of the file at its file URL: None with no file URL, or one
    /// whose type can't be told.
    pub file: Option<&'a str>,
    /// Whether it promises a file.
    pub promise: bool,
    /// The promised file's type, if its source says.
    pub promised: Option<&'a str>,
}

/// How one pasteboard item of a drag is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Taken {
    /// As the file at its file URL.
    File,
    /// As its promise, received once let go.
    Promise,
    /// Not at all: it offers no file.
    Nothing,
}

/// How each of a drag's `items` is taken; `conforms(a, b)` says whether
/// type `a` is a kind of type `b`.
///
/// A file URL beside a promise of the same type is taken as the file (Photos
/// gives a photo's original, or a JPEG of it), and better: nothing is copied. One of another
/// type stands in for the promised file (Photos gives a video's frame as a
/// JPEG), and one promise like that is enough to take the drag's promises,
/// which name the files the source means. A promise with no file URL is
/// taken too; one of an unsaid type leaves its file URL be. Taking the
/// promises leaves an item that promises nothing its file URL: a drag may
/// mix the two.
pub(crate) fn taken(items: &[Offered], conforms: impl Fn(&str, &str) -> bool) -> Vec<Taken> {
    let promises = promises_taken(items, conforms);
    let each = |i: &Offered| match (i.promise, i.file) {
        (true, _) if promises => Taken::Promise,
        (_, Some(_)) => Taken::File,
        _ => Taken::Nothing,
    };
    items.iter().map(each).collect()
}

/// Whether a drag of `items` is taken as its promises rather than its file
/// URLs (`taken`).
fn promises_taken(items: &[Offered], conforms: impl Fn(&str, &str) -> bool) -> bool {
    items.iter().filter(|i| i.promise).any(|i| match (i.file, i.promised) {
        (None, _) => true,
        (Some(file), Some(promised)) => !conforms(file, promised),
        (Some(_), None) => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(items: &[Offered]) -> bool {
        promises_taken(items, |a, b| a == b)
    }

    fn each(items: &[Offered]) -> Vec<Taken> {
        super::taken(items, |a, b| a == b)
    }

    /// A video dragged out of Photos (macOS 26): its file URL is a JPEG of
    /// a frame, in the library's derivatives, beside the promise of the
    /// movie itself (eagle-video.mp4, public.mpeg-4).
    const PHOTOS_VIDEO: Offered = Offered { file: Some("public.jpeg"), promise: true, promised: Some("public.mpeg-4") };

    /// 0.7.2: "Drop in videos from zips and apps. ... one a Mac app hands over only once it's
    /// dropped". `taken`: "One of another type stands in for the promised file (Photos gives a
    /// video's frame as a JPEG)".
    #[test]
    fn a_video_from_photos_is_its_promise_not_its_picture() {
        assert!(taken(&[PHOTOS_VIDEO]));
        assert!(taken(&[PHOTOS_VIDEO, PHOTOS_VIDEO]));
        assert_eq!(each(&[PHOTOS_VIDEO]), [Taken::Promise]);
    }

    /// `taken`: "A file URL beside a promise of the same type is taken as the file (Photos gives a
    /// photo's original, or a JPEG of it), and better: nothing is copied."
    #[test]
    fn a_photo_from_photos_is_its_file() {
        let photo = Offered { file: Some("public.jpeg"), promise: true, promised: Some("public.jpeg") };
        assert!(!taken(&[photo]));
        assert_eq!(each(&[photo]), [Taken::File]);
    }

    /// `taken`: "one promise like that is enough to take the drag's promises, which name the files
    /// the source means": a photo beside a video goes by promises.
    #[test]
    fn a_photo_and_a_video_from_photos_are_their_promises() {
        let photo = Offered { file: Some("public.jpeg"), promise: true, promised: Some("public.jpeg") };
        assert!(taken(&[photo, PHOTOS_VIDEO]));
        assert_eq!(each(&[photo, PHOTOS_VIDEO]), [Taken::Promise, Taken::Promise]);
    }

    /// `taken`: an item with a file URL and no promise is its file.
    #[test]
    fn a_file_from_finder_is_its_file() {
        let file = Offered { file: Some("public.mpeg-4"), ..Default::default() };
        assert!(!taken(&[file]));
        assert_eq!(each(&[file]), [Taken::File]);
    }

    /// `taken`: "A promise with no file URL is taken too" (Safari's image drag, an
    /// NSFilePromiseProvider's: promises alone).
    #[test]
    fn promises_with_no_file_urls_are_taken() {
        assert!(taken(&[Offered { promise: true, promised: Some("public.png"), ..Default::default() }]));
        assert!(taken(&[Offered { promise: true, ..Default::default() }]));
        assert_eq!(each(&[Offered { promise: true, ..Default::default() }]), [Taken::Promise]);
    }

    /// `taken`: "one of an unsaid type leaves its file URL be".
    #[test]
    fn a_promise_of_no_said_type_leaves_the_file() {
        let item = Offered { file: Some("public.mpeg-4"), promise: true, promised: None };
        assert!(!taken(&[item]));
        assert_eq!(each(&[item]), [Taken::File]);
    }

    /// `taken`: a file URL of a kind of the promised type (`conforms`) is the same file as the
    /// promise's.
    #[test]
    fn a_file_conforming_to_the_promised_type_is_its_file() {
        let item = Offered { file: Some("com.apple.quicktime-movie"), promise: true, promised: Some("public.movie") };
        assert!(!promises_taken(&[item], |a, b| a == b || (a == "com.apple.quicktime-movie" && b == "public.movie")));
    }

    /// `taken`: nothing offered is nothing taken.
    #[test]
    fn nothing_offered_is_no_promises() {
        assert!(!taken(&[]));
        assert!(each(&[]).is_empty());
    }

    /// `taken`: "Taking the promises leaves an item that promises nothing its file URL: a drag may
    /// mix the two." (0.7.3 Fixes: "A Mac drop keeps every file. A video dragged in together with
    /// one its app hands over only once dropped is no longer left out.")
    #[test]
    fn a_file_beside_promises_taken_is_its_file() {
        let file = Offered { file: Some("public.mpeg-4"), ..Default::default() };
        assert_eq!(each(&[PHOTOS_VIDEO, file]), [Taken::Promise, Taken::File]);
        let promise = Offered { promise: true, promised: Some("public.mpeg-4"), ..Default::default() };
        assert_eq!(each(&[file, promise]), [Taken::File, Taken::Promise]);
    }

    /// `taken`: promises not taken, each item is its file URL, or `Taken::Nothing` when it offers no
    /// file.
    #[test]
    fn files_not_beside_promises_taken_are_their_files() {
        let photo = Offered { file: Some("public.jpeg"), promise: true, promised: Some("public.jpeg") };
        let file = Offered { file: Some("public.mpeg-4"), ..Default::default() };
        assert_eq!(each(&[photo, file, Offered::default()]), [Taken::File, Taken::File, Taken::Nothing]);
    }

    /// `Taken::Nothing`: "Not at all: it offers no file", also beside promises taken.
    #[test]
    fn an_item_that_offers_no_file_is_taken_as_nothing_beside_promises_too() {
        assert_eq!(each(&[PHOTOS_VIDEO, Offered::default()]), [Taken::Promise, Taken::Nothing]);
    }
}
