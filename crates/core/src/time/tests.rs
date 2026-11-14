//! The specs of time in flicks, at its seam: `Flicks` and `Rate`'s public
//! functions, and the text `timecode` writes and `parse` reads. A spec each,
//! named as it is said (plans/sections-074.md, 3C1).

use super::*;

/// The 24, 25, 30, 48, 50, 60, 90, 100 and 120 a second the module's doc
/// names.
const WHOLE_RATES: [u32; 9] = [24, 25, 30, 48, 50, 60, 90, 100, 120];

/// The NTSC 1001 fractions: 23.976, 29.97, 47.95, 59.94 and 119.88.
const NTSC_RATES: [u32; 5] = [24000, 30000, 48000, 60000, 120000];

/// Every rate the module's doc says a frame is a whole number of flicks at:
/// the two lists above and 44.1 and 48 kHz sound.
fn common_rates() -> Vec<Rate> {
    let mut rates: Vec<Rate> = WHOLE_RATES.iter().map(|&n| Rate::new(n, 1)).collect();
    rates.extend(NTSC_RATES.iter().map(|&n| Rate::new(n, 1001)));
    rates.extend([Rate::new(44100, 1), Rate::new(48000, 1)]);
    rates
}

const R60: Rate = Rate::new(60, 1);
const NTSC30: Rate = Rate::new(30000, 1001);

/// A flick count off by one from frame `n`'s start, either way, is off the
/// grid at this rate.
fn off_grid(rate: Rate) -> [Flicks; 2] {
    [Flicks(Flicks::of_frame(5, rate).0 - 1), Flicks(Flicks::of_frame(5, rate).0 + 1)]
}

/// A flick is a 705,600,000th of a second (time.rs:1 "flicks, 705,600,000 to
/// the second").
#[test]
fn a_second_is_705_600_000_flicks() {
    assert_eq!(PER_SECOND, 705_600_000);
    assert_eq!(Flicks::SECOND, Flicks(705_600_000));
    assert_eq!(Flicks::MILLISECOND, Flicks(705_600));
    assert_eq!(Flicks::from_millis(1000), Flicks::SECOND);
}

/// time.rs:1 "a whole number of them in a frame at every common rate (24, 25,
/// 30, 48, 50, 60, 90, 100 and 120 a second, the NTSC 1001 fractions, and 44.1
/// and 48 kHz sound)".
#[test]
fn a_frame_is_a_whole_number_of_flicks_at_every_common_rate() {
    for rate in common_rates() {
        assert!(rate.exact(), "{rate}");
        let frame = rate.frame();
        // A whole number of flicks, and `num` of them (a second's worth of
        // frames, in `den` seconds) make exactly that many seconds.
        assert_eq!(frame.0 * i64::from(rate.num), PER_SECOND * i64::from(rate.den), "{rate}");
        // So frame n starts at n frames, however far in.
        assert_eq!(Flicks::of_frame(7_654_321, rate), Flicks(7_654_321 * frame.0), "{rate}");
    }
    assert_eq!(R60.frame(), Flicks(11_760_000));
    assert_eq!(NTSC30.frame(), Flicks(23_543_520));
}

/// time.rs:1 "a whole number of them in a frame at every common rate", and
/// `Rate::exact`: "Whether a frame at this rate is a whole number of flicks".
#[test]
fn a_rate_is_exact_when_its_frame_is_a_whole_number_of_flicks() {
    // 29.97 taken literally isn't; nor is 11 a second.
    assert!(!Rate::new(2997, 100).exact());
    assert!(!Rate::new(11, 1).exact());
    assert!(!Rate::new(23976, 1000).exact());
    // A third of a frame at 100 a second is whole, so 300 a second is.
    assert!(Rate::new(300, 1).exact());
    for rate in [Rate::new(2997, 100), Rate::new(11, 1), Rate::new(23976, 1000), Rate::new(60, 1), NTSC30] {
        let whole = Flicks::of_frame(1, rate).0 * i64::from(rate.num) == PER_SECOND * i64::from(rate.den);
        assert_eq!(rate.exact(), whole, "{rate}");
    }
}

/// `Rate::frame`: "One frame at this rate (`exact` or nearest)".
#[test]
fn a_rates_frame_is_exact_or_the_nearest_flick() {
    assert_eq!(R60.frame(), Flicks::of_frame(1, R60));
    let odd = Rate::new(2997, 100);
    assert_eq!(odd.frame(), Flicks((PER_SECOND as f64 * 100.0 / 2997.0).round() as i64));
}

/// 0.4.1 Fixes: "Videos at 23.976, 29.97 or 59.94 frames a second showed one
/// frame twice every few minutes, and stepping frame by frame drifted." The
/// frame a moment shows is the frame number, over hours of frames, with every
/// frame shown for exactly its length and none twice.
#[test]
fn stepping_frame_by_frame_at_ntsc_rates_never_drifts_or_repeats_a_frame() {
    for num in [24000, 30000, 60000] {
        let rate = Rate::new(num, 1001);
        let frame = rate.frame();
        // Three hours of frames.
        let frames = i64::from(num) * 3 * 3600 / 1001;
        let mut last = Flicks::of_frame(0, rate);
        assert_eq!(last, Flicks::ZERO);
        for n in 0..=frames {
            let start = Flicks::of_frame(n, rate);
            assert_eq!(start, Flicks(n * frame.0), "{rate}: frame {n} starts where {n} frames add up");
            assert_eq!(start.frame(rate), n, "{rate}: frame {n}'s first flick");
            let under_half = start + Flicks((frame.0 - 1) / 2);
            assert_eq!(under_half.frame(rate), n, "{rate}: frame {n} lasts to half a frame on");
            let half = start + Flicks(frame.0 / 2 + frame.0 % 2);
            assert_eq!(half.frame(rate), n + 1, "{rate}: and the next frame starts there");
            if n > 0 {
                assert_eq!(start - last, frame, "{rate}: step {n}");
            }
            last = start;
        }
    }
}

/// `Flicks::of_frame`: "Frame `n` at `rate`, from zero: exact at every rate
/// flicks represent, else to the nearest flick."
#[test]
fn frame_n_starts_at_n_frames_from_zero_to_the_nearest_flick() {
    // 29.97's frame 1800 is 60.06 s exactly.
    assert_eq!(Flicks::of_frame(1800, NTSC30), Flicks::from_millis(60_060));
    assert_eq!(Flicks::of_frame(0, NTSC30), Flicks::ZERO);
    assert_eq!(Flicks::of_frame(1, NTSC30), Flicks(23_543_520));
    // A rate flicks can't hold exactly: whole seconds still land on whole
    // seconds, and a frame is the nearest flick to its true start.
    let odd = Rate::new(2997, 100);
    assert_eq!(Flicks::of_frame(2997, odd).seconds(), 100.0);
    for n in [1, 2, 3, 1000, 123_456] {
        let exact = n as f64 * PER_SECOND as f64 * 100.0 / 2997.0;
        assert_eq!(Flicks::of_frame(n, odd).0, exact.round() as i64, "frame {n}");
    }
}

/// `Flicks::frame`: "The frame at `rate` this moment is nearest to."
#[test]
fn a_moment_is_in_the_frame_it_is_nearest_to() {
    let t = Flicks::from_seconds(7.0 / 60.0 + 0.004);
    assert_eq!(t.frame(R60), 7);
    assert_eq!(Flicks::from_seconds(7.0 / 60.0 + 0.012).frame(R60), 8, "nearer the next");
    assert_eq!(Flicks::from_millis(60_060).frame(NTSC30), 1800);
    // Halfway rounds up (time.rs `div_round`: "Nearest, halves away from
    // zero"), and a negative half away from zero too.
    let half = Flicks(R60.frame().0 / 2);
    assert_eq!(half.frame(R60), 1);
    assert_eq!(Flicks(-half.0).frame(R60), -1);
    assert_eq!(Flicks(half.0 - 1).frame(R60), 0);
    assert_eq!(Flicks(-(half.0 - 1)).frame(R60), 0);
    assert_eq!(Flicks(-R60.frame().0).frame(R60), -1);
}

/// `Flicks::frame` and `of_frame` at every common rate: the frame number a
/// start gives back is the number it came from.
#[test]
fn a_frames_start_is_in_that_frame_at_every_common_rate() {
    for rate in common_rates() {
        for n in [0, 1, 2, 29, 30, 59, 60, 1001, 100_000, 5_000_000] {
            assert_eq!(Flicks::of_frame(n, rate).frame(rate), n, "{rate} frame {n}");
        }
    }
}

/// `Flicks::snap`: "This moment moved to the nearest frame at `rate`", and
/// time.rs:6 "the writer snaps a cut to its source's frames".
#[test]
fn a_moment_snaps_to_the_nearest_frame() {
    let t = Flicks::from_seconds(7.0 / 60.0 + 0.004);
    assert_eq!(t.snap(R60), Flicks::of_frame(7, R60));
    assert_eq!(Flicks::from_seconds(7.0 / 60.0 + 0.012).snap(R60), Flicks::of_frame(8, R60));
    assert_eq!(Flicks::from_millis(60_070).snap(NTSC30), Flicks::from_millis(60_060));
    assert_eq!(Flicks(R60.frame().0 / 2).snap(R60), R60.frame(), "halfway goes up");
    for rate in common_rates() {
        let frame = rate.frame();
        for t in [Flicks(1), Flicks::from_millis(1234), Flicks::from_seconds(3723.4567), Flicks(frame.0 * 5 + 3)] {
            let snapped = t.snap(rate);
            assert!(snapped.on_frame(rate), "{rate}: {t:?}");
            assert!((snapped - t).0.abs() * 2 <= frame.0, "{rate}: {t:?} moved no more than half a frame");
            assert_eq!(snapped.snap(rate), snapped, "{rate}: a frame stays where it is");
        }
    }
}

/// `Flicks::on_frame`: "Whether this moment is on a frame at `rate`."
#[test]
fn a_moment_is_on_a_frame_only_at_a_frames_start() {
    for rate in common_rates() {
        assert!(Flicks::ZERO.on_frame(rate), "{rate}");
        assert!(Flicks::of_frame(12_345, rate).on_frame(rate), "{rate}");
        for off in off_grid(rate) {
            assert!(!off.on_frame(rate), "{rate}: {off:?} is a flick from a frame");
        }
    }
    // 65.25 s is on a frame at 60 a second (frame 3915) and not at 30000/1001.
    assert!(Flicks::from_millis(65_250).on_frame(R60));
    assert!(!Flicks::from_millis(65_250).on_frame(NTSC30));
    // The frame a moment is nearest to isn't enough.
    assert!(!Flicks::from_seconds(7.0 / 60.0 + 0.004).on_frame(R60));
}

/// `Flicks::timecode`: "As timecode at `rate`, `h:mm:ss:ff` (the frame within
/// the second)".
#[test]
fn timecode_at_a_rate_is_hours_minutes_seconds_and_the_frame() {
    let t = Flicks::from_seconds(3723.0) + Flicks::of_frame(30, R60);
    assert_eq!(t.timecode(Some(R60)), "1:02:03:30");
    assert_eq!(Flicks::ZERO.timecode(Some(R60)), "0:00:00:00");
    assert_eq!(Flicks::from_seconds(59.0).timecode(Some(R60)), "0:00:59:00");
    assert_eq!((Flicks::from_seconds(59.0) + Flicks::of_frame(59, R60)).timecode(Some(R60)), "0:00:59:59");
    assert_eq!(Flicks::from_millis(65_250).timecode(Some(R60)), "0:01:05:15");
    // Hours are as many as there are, minutes and seconds two digits, and the
    // frame two digits too.
    assert_eq!(
        (Flicks::from_seconds(25.0 * 3600.0 + 61.0) + Flicks::of_frame(5, R60)).timecode(Some(R60)),
        "25:01:01:05"
    );
    // At 29.97, a frame that starts within its second.
    assert_eq!(Flicks::of_frame(1, NTSC30).timecode(Some(NTSC30)), "0:00:00:01");
    assert_eq!(Flicks::of_frame(29, NTSC30).timecode(Some(NTSC30)), "0:00:00:29");
}

/// `Flicks::timecode`: "or without a rate `h:mm:ss.mmm`", and `Display`
/// writes it so.
#[test]
fn timecode_without_a_rate_is_hours_minutes_seconds_and_milliseconds() {
    let t = Flicks::from_seconds(3723.5);
    assert_eq!(t.timecode(None), "1:02:03.500");
    assert_eq!(t.to_string(), "1:02:03.500");
    assert_eq!(Flicks::ZERO.timecode(None), "0:00:00.000");
    assert_eq!(Flicks::from_millis(1).timecode(None), "0:00:00.001");
    assert_eq!(Flicks::from_millis(25 * 3_600_000 + 61_007).timecode(None), "25:01:01.007");
    assert_eq!(Flicks(-PER_SECOND).timecode(None), "-0:00:01.000");
}

/// `Flicks::timecode`: "A moment off the grid is shown to the millisecond
/// either way, so the text never claims a frame it isn't on."
#[test]
fn timecode_off_the_grid_is_shown_to_the_millisecond() {
    // Off the grid at 30 a second, on it at 60.
    assert_eq!(Flicks::from_millis(65_250).timecode(Some(Rate::new(30, 1))), "0:01:05.250");
    // A flick off a frame, at 60 a second.
    let near = Flicks::of_frame(30, R60) + Flicks(1);
    assert_eq!(near.timecode(Some(R60)), "0:00:00.500");
    // Rounded to the millisecond, not cut.
    assert_eq!(Flicks::from_seconds(0.0016).timecode(None), "0:00:00.002");
    assert_eq!(Flicks::from_seconds(0.0014).timecode(None), "0:00:00.001");
}

/// `Flicks::timecode`: frames are shown only at a rate that is `exact`; at one
/// that isn't, "the text never claims a frame it isn't on".
#[test]
fn timecode_at_an_inexact_rate_shows_milliseconds() {
    let odd = Rate::new(2997, 100);
    assert_eq!(Flicks::of_frame(1, odd).timecode(Some(odd)), "0:00:00.033");
    assert_eq!(Flicks::ZERO.timecode(Some(odd)), "0:00:00.000");
    assert_eq!(Flicks::of_frame(2997, odd).timecode(Some(odd)), "0:01:40.000");
}

/// `Flicks::timecode`: "so the text never claims a frame it isn't on": at
/// every common rate, a timecode with a frame in it reads back as the moment
/// it was written from.
#[test]
fn timecode_never_claims_a_frame_the_moment_isnt_on() {
    for rate in common_rates() {
        for n in (0..4000).chain([99_999, 1_000_001]) {
            let t = Flicks::of_frame(n, rate);
            let text = t.timecode(Some(rate));
            if text.matches(':').count() == 3 {
                assert_eq!(Flicks::parse(&text, Some(rate)), Some(t), "{rate} frame {n}: {text}");
            }
        }
    }
}

/// `Flicks::timecode`: "without a rate `h:mm:ss.mmm`": three digits of
/// milliseconds, so a moment less than half a millisecond short of a second is
/// the next second, not a thousand milliseconds.
#[test]
fn timecode_milliseconds_are_three_digits() {
    assert_eq!(Flicks::from_seconds(0.9996).timecode(None), "0:00:01.000");
    assert_eq!(Flicks::from_seconds(59.9996).timecode(None), "0:01:00.000");
    // And at 24000/1001 a frame off the second's grid: frame 1990 is 82.9958 s.
    let rate = Rate::new(24000, 1001);
    let text = Flicks::of_frame(1990, rate).timecode(Some(rate));
    assert_eq!(text.rsplit_once('.').map(|(_, ms)| ms.len()), Some(3), "{text}");
}

/// `Flicks::parse`: "`h:mm:ss:ff` at `rate`, `h:mm:ss.mmm`, or `m:ss` and `ss`
/// with the hours or minutes left off, as `timecode` writes them".
#[test]
fn parse_reads_the_forms_timecode_writes() {
    assert_eq!(Flicks::parse("1:02:03:30", Some(R60)), Some(Flicks::from_seconds(3723.5)));
    assert_eq!(Flicks::parse("1:02:03.500", None), Some(Flicks::from_seconds(3723.5)));
    assert_eq!(Flicks::parse("1:02:03.500", Some(R60)), Some(Flicks::from_seconds(3723.5)));
    assert_eq!(Flicks::parse("2:03.250", None), Some(Flicks::from_millis(123_250)));
    assert_eq!(Flicks::parse("1:02:03", None), Some(Flicks::from_seconds(3723.0)));
    assert_eq!(Flicks::parse("2:03", None), Some(Flicks::from_seconds(123.0)));
    assert_eq!(Flicks::parse("45", None), Some(Flicks::from_seconds(45.0)));
    assert_eq!(Flicks::parse("0", None), Some(Flicks::ZERO));
    assert_eq!(Flicks::parse("1:00:00:00", Some(NTSC30)), Some(Flicks::from_seconds(3600.0)));
    assert_eq!(Flicks::parse("0:00:00:29", Some(NTSC30)), Some(Flicks::of_frame(29, NTSC30)));
    assert_eq!(Flicks::parse("-0:00:01.000", None), Some(Flicks(-PER_SECOND)));
}

/// `Flicks::parse`: "as `timecode` writes them": what `timecode` wrote reads
/// back as the same moment, in the frame form at its rate and in the
/// millisecond form without one.
#[test]
fn timecode_parses_back_to_the_same_flicks() {
    for t in [Flicks::ZERO, Flicks::of_frame(30, R60), Flicks::from_seconds(3723.0) + Flicks::of_frame(59, R60)] {
        assert_eq!(Flicks::parse(&t.timecode(Some(R60)), Some(R60)), Some(t), "{}", t.timecode(Some(R60)));
    }
    for t in [Flicks::ZERO, Flicks::from_millis(1), Flicks::from_millis(3_723_983), Flicks::from_millis(-90_500)] {
        assert_eq!(Flicks::parse(&t.timecode(None), None), Some(t), "{}", t.timecode(None));
        assert_eq!(Flicks::parse(&t.timecode(Some(R60)), Some(R60)), Some(t), "{}", t.timecode(Some(R60)));
    }
    // Off a frame at 60 a second, shown to the millisecond and read back so.
    let off = Flicks::from_millis(1234) + Flicks(1);
    assert_eq!(Flicks::parse(&off.timecode(Some(R60)), Some(R60)), Some(Flicks::from_millis(1234)));
}

/// `Flicks::parse`: "None if it isn't one (frames without a rate included)".
#[test]
fn parse_is_none_for_text_that_isnt_a_timecode() {
    assert_eq!(Flicks::parse("1:02:03:30", None), None, "frames need a rate");
    for text in [
        "",
        " ",
        "abc",
        "-",
        "1:-2",
        "1:02:03:30:1",
        "1:2:3:4:5",
        "1::2",
        ":",
        "1:",
        ":30",
        "1.",
        ".",
        ".5.5",
        "1.5x",
        "1:02.3:4",
        "1.-5",
        "--1",
        "1:02:03:x",
        "1 2",
    ] {
        assert_eq!(Flicks::parse(text, Some(R60)), None, "{text:?}");
        assert_eq!(Flicks::parse(text, None), None, "{text:?}");
    }
}

/// `Flicks::parse`: "None if it isn't one": a frame or a part with a sign
/// isn't a timecode (only a leading "-" is).
#[test]
fn parse_refuses_a_sign_inside_the_text() {
    for text in ["1:02:03:+1", "1:02:03:-1", "+5", "1:+2", "1:-2", "0:00:00:+1", "--5"] {
        assert_eq!(Flicks::parse(text, Some(R60)), None, "{text:?}");
    }
}

/// `Flicks::from_seconds`: "`seconds` to the nearest flick (under two
/// nanoseconds off)", and `from_millis` and `seconds` the other way.
#[test]
fn seconds_are_the_nearest_flick() {
    assert_eq!(Flicks::from_seconds(1.5), Flicks(PER_SECOND + PER_SECOND / 2));
    assert_eq!(Flicks::from_seconds(0.0), Flicks::ZERO);
    assert_eq!(Flicks::from_seconds(-1.5), Flicks(-(PER_SECOND + PER_SECOND / 2)));
    assert_eq!(Flicks::from_millis(3_723_125).seconds(), 3723.125);
    assert_eq!(Flicks::from_seconds(86_400.0).0, 86_400 * PER_SECOND, "a day is well within range");
    for s in [1.0 / 3.0, 0.123_456_789, 7.7, 1_234.567_891, 86_399.999_999_9] {
        let off = (Flicks::from_seconds(s).seconds() - s).abs();
        assert!(off < 2e-9, "{s}: {off} s off");
    }
}

/// time.rs:7 "A value from a player's seconds (an embed before its footage is
/// here) is exact to the millisecond, a whole number of flicks too": every
/// millisecond over a day's first hours, as a float of seconds, is that many
/// whole milliseconds of flicks, and reads back as the same milliseconds.
#[test]
fn a_players_seconds_are_exact_to_the_millisecond() {
    for ms in (0..12_000_000).step_by(7).chain([86_399_999]) {
        let flicks = Flicks::from_seconds(ms as f64 / 1000.0);
        assert_eq!(flicks, Flicks::from_millis(ms), "{ms} ms");
        assert_eq!(flicks.0 % Flicks::MILLISECOND.0, 0, "{ms} ms is whole flicks");
        assert_eq!((flicks.seconds() * 1000.0).round() as i64, ms, "{ms} ms read back");
    }
}

/// `Flicks` is "A moment or a length, in flicks": adding and subtracting two
/// is adding and subtracting their flicks.
#[test]
fn moments_and_lengths_add_and_subtract_as_flicks() {
    assert_eq!(Flicks(5) + Flicks(7), Flicks(12));
    assert_eq!(Flicks(5) - Flicks(7), Flicks(-2));
    assert_eq!(Flicks::SECOND + Flicks::MILLISECOND, Flicks::from_millis(1001));
    assert_eq!(Flicks::SECOND - Flicks::SECOND, Flicks::ZERO);
}

/// `Rate::from_fps`: "a whole number, an NTSC fraction, or else to the
/// thousandth. None for nothing or nonsense." Also `local::frame_rate`'s
/// 29.97 as 30000/1001.
#[test]
fn a_rate_is_made_from_what_ffmpeg_reports() {
    for fps in [24, 25, 30, 48, 50, 60, 120, 1] {
        assert_eq!(Rate::from_fps(f64::from(fps)), Some(Rate::new(fps, 1)));
    }
    for num in [24000, 30000, 48000, 60000, 120000] {
        assert_eq!(Rate::from_fps(f64::from(num) / 1001.0), Some(Rate::new(num, 1001)), "{num}/1001");
    }
    assert_eq!(Rate::from_fps(33.333), Some(Rate::new(33333, 1000)));
    assert_eq!(Rate::from_fps(0.001), Some(Rate::new(1, 1000)));
    assert_eq!(Rate::from_fps(99_999.0), Some(Rate::new(99_999, 1)));
}

/// `Rate::from_fps`: "None for nothing or nonsense."
#[test]
fn no_rate_comes_from_nothing_or_nonsense() {
    for fps in [0.0, -30.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 100_000.0, 1e9, 0.000_1] {
        assert_eq!(Rate::from_fps(fps), None, "{fps}");
    }
}

/// `Rate` is "frames a second in lowest terms: 30000/1001 for 29.97", so a
/// rate from 12.5 is 25/2.
#[test]
fn a_rate_from_fps_is_in_lowest_terms() {
    assert_eq!(Rate::from_fps(12.5), Some(Rate::new(25, 2)));
    assert_eq!(Rate::from_fps(29.97), Some(Rate::new(2997, 100)));
    assert_eq!(Rate::from_fps(33.333), Some(Rate::new(33333, 1000)));
}

/// `Rate` is "frames a second": `fps` gives them as a number.
#[test]
fn a_rate_is_frames_a_second() {
    assert_eq!(R60.fps(), 60.0);
    assert_eq!(Rate::new(25, 2).fps(), 12.5);
    assert!((NTSC30.fps() - 29.970_03).abs() < 1e-5);
}
