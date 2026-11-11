//! Adding VODs, and refreshing their details.

use super::*;

/// A second Refresh details of a VOD while its first is under way gets
/// nothing to do; another VOD's goes ahead, and once the first ends, its next does.
#[test]
fn one_refresh_of_a_vod_at_a_time() {
    let under_way = Mutex::default();
    let first = Refreshing::claim(&under_way, 7).unwrap();
    assert!(Refreshing::claim(&under_way, 7).is_none());
    assert!(Refreshing::claim(&under_way, 8).is_some());
    drop(first);
    assert!(Refreshing::claim(&under_way, 7).is_some());
}
