//! Which of a drag's two offers of its files to take on macOS, the file
//! URLs or the file promises, told from what each pasteboard item says it
//! is. Kept apart from the AppKit calls so that it's tested everywhere.

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

/// Whether a drag of `items` is taken as its promises rather than its file
/// URLs; `conforms(a, b)` says whether type `a` is a kind of type `b`.
///
/// A file URL beside a promise of the same type is taken as the file (Photos
/// gives a photo's original, or a JPEG of it), and better: nothing is copied. One of another
/// type stands in for the promised file (Photos gives a video's frame as a
/// JPEG), and one promise like that is enough to take the drag's promises,
/// which name the files the source means. A promise with no file URL is
/// taken too; one of an unsaid type leaves its file URL be.
pub(crate) fn promises_taken(items: &[Offered], conforms: impl Fn(&str, &str) -> bool) -> bool {
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

    /// A video dragged out of Photos (macOS 26): its file URL is a JPEG of
    /// a frame, in the library's derivatives, beside the promise of the
    /// movie itself (eagle-video.mp4, public.mpeg-4).
    const PHOTOS_VIDEO: Offered = Offered { file: Some("public.jpeg"), promise: true, promised: Some("public.mpeg-4") };

    #[test]
    fn a_video_from_photos_is_its_promise_not_its_picture() {
        assert!(taken(&[PHOTOS_VIDEO]));
        assert!(taken(&[PHOTOS_VIDEO, PHOTOS_VIDEO]));
    }

    /// A photo dragged out of Photos: its file URL is the original in the
    /// library or a JPEG of it, of the promise's own type either way.
    #[test]
    fn a_photo_from_photos_is_its_file() {
        let photo = Offered { file: Some("public.jpeg"), promise: true, promised: Some("public.jpeg") };
        assert!(!taken(&[photo]));
    }

    /// A photo and a video together: the video's promise is wanted, so the
    /// drag is taken as promises (the photo's promise gives its original).
    #[test]
    fn a_photo_and_a_video_from_photos_are_their_promises() {
        let photo = Offered { file: Some("public.jpeg"), promise: true, promised: Some("public.jpeg") };
        assert!(taken(&[photo, PHOTOS_VIDEO]));
    }

    #[test]
    fn a_file_from_finder_is_its_file() {
        let file = Offered { file: Some("public.mpeg-4"), ..Default::default() };
        assert!(!taken(&[file]));
    }

    /// Safari's image drag, an NSFilePromiseProvider's: promises alone.
    #[test]
    fn promises_with_no_file_urls_are_taken() {
        assert!(taken(&[Offered { promise: true, promised: Some("public.png"), ..Default::default() }]));
        assert!(taken(&[Offered { promise: true, ..Default::default() }]));
    }

    /// A promise of an unsaid type beside a file URL: the file, as before.
    #[test]
    fn a_promise_of_no_said_type_leaves_the_file() {
        assert!(!taken(&[Offered { file: Some("public.mpeg-4"), promise: true, promised: None }]));
    }

    /// A file of a subtype of the promised one is the same file.
    #[test]
    fn a_file_conforming_to_the_promised_type_is_its_file() {
        let item = Offered { file: Some("com.apple.quicktime-movie"), promise: true, promised: Some("public.movie") };
        assert!(!promises_taken(&[item], |a, b| a == b || (a == "com.apple.quicktime-movie" && b == "public.movie")));
    }

    #[test]
    fn nothing_offered_is_no_promises() {
        assert!(!taken(&[]));
    }
}
