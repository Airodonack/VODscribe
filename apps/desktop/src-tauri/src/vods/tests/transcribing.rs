//! Asking for a VOD's transcription.

use super::*;

#[test]
fn a_stretch_asked_for_is_within_the_vod() {
    assert_eq!(check_stretch(Some(600.0), 100.0, Some(200.0)), Ok(()));
    assert_eq!(check_stretch(Some(600.0), 100.0, None), Ok(()));
    assert_eq!(check_stretch(None, 100.0, None), Ok(()), "a length not known");
    assert!(check_stretch(Some(600.0), 600.0, None).is_err(), "past the end");
    assert!(check_stretch(Some(600.0), 200.0, Some(100.0)).is_err(), "empty");
    assert!(check_stretch(Some(600.0), f64::NAN, None).is_err());
    // A length its site or ffmpeg gave as 0 isn't known: all of it can be asked for.
    assert_eq!(check_stretch(Some(0.0), 0.0, None), Ok(()));
}
