//! Time as the record keeps it: flicks, 705,600,000 to the second, a whole
//! number of them in a frame at every common rate (24, 25, 30, 48, 50, 60,
//! 90, 100 and 120 a second, the NTSC 1001 fractions, and 44.1 and 48 kHz
//! sound). One unit for a piece's in and out points, a position on a clip's
//! timeline, a mark's moment and a keyframe's time, so no arithmetic mixes
//! units. A rate is a grid, not a unit: the writer snaps a cut to its
//! source's frames when the rate is known (`Flicks::snap`) and a position to
//! the clip's. A value from a player's seconds (an embed before its footage
//! is here) is exact to the millisecond, a whole number of flicks too.

use std::fmt;

/// Flicks in a second.
pub const PER_SECOND: i64 = 705_600_000;

/// A moment or a length, in flicks.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Flicks(pub i64);

impl Flicks {
    pub const ZERO: Flicks = Flicks(0);
    pub const SECOND: Flicks = Flicks(PER_SECOND);
    pub const MILLISECOND: Flicks = Flicks(PER_SECOND / 1000);

    /// `seconds` to the nearest flick (under two nanoseconds off).
    pub fn from_seconds(seconds: f64) -> Flicks {
        Flicks((seconds * PER_SECOND as f64).round() as i64)
    }

    pub fn from_millis(ms: i64) -> Flicks {
        Flicks(ms * Self::MILLISECOND.0)
    }

    pub fn seconds(self) -> f64 {
        self.0 as f64 / PER_SECOND as f64
    }

    /// Frame `n` at `rate`, from zero: exact at every rate flicks represent,
    /// else to the nearest flick.
    pub fn of_frame(n: i64, rate: Rate) -> Flicks {
        // n * PER_SECOND * den / num, rounded.
        let num = i128::from(rate.num);
        let scaled = i128::from(n) * i128::from(PER_SECOND) * i128::from(rate.den);
        Flicks(div_round(scaled, num) as i64)
    }

    /// The frame at `rate` this moment is nearest to.
    pub fn frame(self, rate: Rate) -> i64 {
        let den = i128::from(PER_SECOND) * i128::from(rate.den);
        div_round(i128::from(self.0) * i128::from(rate.num), den) as i64
    }

    /// This moment moved to the nearest frame at `rate`.
    pub fn snap(self, rate: Rate) -> Flicks {
        Flicks::of_frame(self.frame(rate), rate)
    }

    /// Whether this moment is on a frame at `rate`.
    pub fn on_frame(self, rate: Rate) -> bool {
        self.snap(rate) == self
    }

    /// As timecode at `rate`, `h:mm:ss:ff` (the frame within the second), or
    /// without a rate `h:mm:ss.mmm`. A moment off the grid is shown to the
    /// millisecond either way, so the text never claims a frame it isn't on.
    pub fn timecode(self, rate: Option<Rate>) -> String {
        let (sign, t) = if self.0 < 0 { ("-", Flicks(-self.0)) } else { ("", self) };
        let whole = t.0 / PER_SECOND;
        let rest = Flicks(t.0 % PER_SECOND);
        let (h, m, s) = (whole / 3600, whole / 60 % 60, whole % 60);
        match rate.filter(|r| rest.on_frame(*r) && r.exact()) {
            Some(rate) => format!("{sign}{h}:{m:02}:{s:02}:{:02}", rest.frame(rate)),
            None => {
                format!("{sign}{h}:{m:02}:{s:02}.{:03}", div_round(i128::from(rest.0), i128::from(Self::MILLISECOND.0)))
            }
        }
    }

    /// `h:mm:ss:ff` at `rate`, `h:mm:ss.mmm`, or `m:ss` and `ss` with the
    /// hours or minutes left off, as `timecode` writes them; None if it
    /// isn't one (frames without a rate included).
    pub fn parse(text: &str, rate: Option<Rate>) -> Option<Flicks> {
        let text = text.trim();
        let (negative, text) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        // The fraction: milliseconds after a dot, or frames as a fourth part.
        let (clock, millis, frames) = if let Some((clock, ms)) = text.split_once('.') {
            if ms.is_empty() || !ms.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            (clock, Some(format!("{ms:0<3}")[..3].parse::<i64>().ok()?), None)
        } else if text.matches(':').count() == 3 {
            let (clock, ff) = text.rsplit_once(':')?;
            (clock, None, Some(ff.parse::<i64>().ok()?))
        } else {
            (text, None, None)
        };
        let parts: Vec<i64> = clock.split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
        if parts.is_empty() || parts.len() > 3 || parts.iter().any(|&p| p < 0) {
            return None;
        }
        let seconds = parts.iter().fold(0, |acc, &p| acc * 60 + p);
        let mut flicks = seconds * PER_SECOND;
        if let Some(ff) = frames {
            flicks += Flicks::of_frame(ff, rate?).0;
        }
        if let Some(ms) = millis {
            flicks += ms * Self::MILLISECOND.0;
        }
        Some(Flicks(if negative { -flicks } else { flicks }))
    }
}

fn div_round(n: i128, d: i128) -> i128 {
    // Nearest, halves away from zero.
    let (q, r) = (n / d, n % d);
    if 2 * r.abs() >= d.abs() { q + n.signum() * d.signum() } else { q }
}

impl fmt::Display for Flicks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.timecode(None))
    }
}

impl std::ops::Add for Flicks {
    type Output = Flicks;
    fn add(self, rhs: Flicks) -> Flicks {
        Flicks(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Flicks {
    type Output = Flicks;
    fn sub(self, rhs: Flicks) -> Flicks {
        Flicks(self.0 - rhs.0)
    }
}

/// A frame rate, as frames a second in lowest terms: 30000/1001 for 29.97.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Rate {
    pub num: u32,
    pub den: u32,
}

impl Rate {
    pub const fn new(num: u32, den: u32) -> Rate {
        Rate { num, den }
    }

    /// The rate ffmpeg reports as `fps` (`local::frame_rate`, which already
    /// gives 29.97 as 30000/1001 exactly): a whole number, an NTSC fraction,
    /// or else to the thousandth. None for nothing or nonsense.
    pub fn from_fps(fps: f64) -> Option<Rate> {
        if !(fps.is_finite() && fps > 0.0 && fps < 100_000.0) {
            return None;
        }
        let near = |value: f64, den: u32| {
            let num = (value * f64::from(den)).round();
            ((num / f64::from(den) - value).abs() < 1e-6).then_some(Rate::new(num as u32, den))
        };
        near(fps, 1).or_else(|| near(fps, 1001).filter(|r| r.num % 1000 == 0)).or_else(|| near(fps, 1000))
    }

    pub fn fps(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }

    /// Whether a frame at this rate is a whole number of flicks.
    pub fn exact(self) -> bool {
        (PER_SECOND * i64::from(self.den)) % i64::from(self.num) == 0
    }

    /// One frame at this rate (`exact` or nearest).
    pub fn frame(self) -> Flicks {
        Flicks::of_frame(1, self)
    }
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 { write!(f, "{} fps", self.num) } else { write!(f, "{}/{} fps", self.num, self.den) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_rates_are_whole_flicks() {
        for fps in [24, 25, 30, 48, 50, 60, 90, 100, 120] {
            assert!(Rate::new(fps, 1).exact(), "{fps}");
        }
        for num in [24000, 30000, 60000] {
            assert!(Rate::new(num, 1001).exact(), "{num}/1001");
        }
        assert_eq!(Rate::new(60, 1).frame(), Flicks(11_760_000));
        assert_eq!(Rate::new(30000, 1001).frame(), Flicks(23_543_520));
        // Sound at 48 kHz, and 44.1.
        assert!(Rate::new(48000, 1).exact() && Rate::new(44100, 1).exact());
        assert!(!Rate::new(2997, 100).exact(), "29.97 taken literally isn't");
        assert!(Rate::new(100, 3).exact(), "a third of a frame is 7,056,000 flicks");
    }

    #[test]
    fn rates_come_from_what_ffmpeg_shows() {
        assert_eq!(Rate::from_fps(30.0), Some(Rate::new(30, 1)));
        assert_eq!(Rate::from_fps(30000.0 / 1001.0), Some(Rate::new(30000, 1001)));
        assert_eq!(Rate::from_fps(24000.0 / 1001.0), Some(Rate::new(24000, 1001)));
        assert_eq!(Rate::from_fps(33.333), Some(Rate::new(33333, 1000)));
        assert_eq!(Rate::from_fps(0.0), None);
        assert_eq!(Rate::from_fps(f64::NAN), None);
        assert_eq!(Rate::new(30000, 1001).to_string(), "30000/1001 fps");
        assert_eq!(Rate::new(60, 1).to_string(), "60 fps");
    }

    #[test]
    fn seconds_round_trip_to_the_millisecond() {
        assert_eq!(Flicks::from_seconds(1.5), Flicks(PER_SECOND + PER_SECOND / 2));
        assert_eq!(Flicks::from_millis(3_723_125).seconds(), 3723.125);
        assert_eq!(Flicks::from_seconds(0.001), Flicks::MILLISECOND);
        // A day is well within range.
        assert_eq!(Flicks::from_seconds(86_400.0).0, 86_400 * PER_SECOND);
    }

    #[test]
    fn snapping_moves_to_the_nearest_frame() {
        let r60 = Rate::new(60, 1);
        let t = Flicks::from_seconds(1.0 / 60.0 * 7.0 + 0.004);
        assert_eq!(t.frame(r60), 7);
        assert_eq!(t.snap(r60), Flicks::of_frame(7, r60));
        assert!(t.snap(r60).on_frame(r60) && !t.on_frame(r60));
        // Halfway rounds up.
        let half = Flicks(r60.frame().0 / 2);
        assert_eq!(half.frame(r60), 1);
        // NTSC: frame 1800 of 29.97 is 60.06 s exactly.
        let ntsc = Rate::new(30000, 1001);
        assert_eq!(Flicks::of_frame(1800, ntsc), Flicks::from_millis(60_060));
        assert_eq!(Flicks::from_millis(60_060).frame(ntsc), 1800);
        // A rate flicks can't hold exactly still snaps, to the nearest flick.
        let odd = Rate::new(2997, 100);
        assert_eq!(Flicks::of_frame(2997, odd).seconds(), 100.0);
        assert_eq!(Flicks::of_frame(1, odd).0, (PER_SECOND as f64 * 100.0 / 2997.0).round() as i64);
    }

    #[test]
    fn timecode_shows_frames_on_the_grid_and_milliseconds_off_it() {
        let r60 = Rate::new(60, 1);
        let t = Flicks::from_seconds(3723.0) + Flicks::of_frame(30, r60);
        assert_eq!(t.timecode(Some(r60)), "1:02:03:30");
        assert_eq!(t.timecode(None), "1:02:03.500");
        assert_eq!(Flicks::from_millis(65_250).timecode(Some(Rate::new(30, 1))), "0:01:05.250", "not on a frame");
        assert_eq!(Flicks::from_millis(65_250).timecode(Some(Rate::new(60, 1))), "0:01:05:15");
        assert_eq!(Flicks::ZERO.timecode(Some(r60)), "0:00:00:00");
        assert_eq!(Flicks(-Flicks::SECOND.0).timecode(None), "-0:00:01.000");
        let odd = Rate::new(2997, 100);
        assert_eq!(Flicks::of_frame(1, odd).timecode(Some(odd)), "0:00:00.033", "no frames shown at an inexact rate");
        // Rounded to the millisecond, not cut.
        assert_eq!(Flicks::from_seconds(0.0016).timecode(None), "0:00:00.002");
    }

    #[test]
    fn timecode_parses_back_to_the_same_flicks() {
        let r60 = Rate::new(60, 1);
        // On a frame, through the frame form; on a millisecond, through the other.
        for t in [Flicks::ZERO, Flicks::of_frame(30, r60), Flicks::from_seconds(3723.0) + Flicks::of_frame(59, r60)] {
            assert_eq!(Flicks::parse(&t.timecode(Some(r60)), Some(r60)), Some(t), "{}", t.timecode(Some(r60)));
        }
        for t in [Flicks::ZERO, Flicks::from_millis(1), Flicks::from_millis(3_723_983)] {
            assert_eq!(Flicks::parse(&t.timecode(None), None), Some(t), "{}", t.timecode(None));
            assert_eq!(Flicks::parse(&t.timecode(Some(r60)), Some(r60)), Some(t), "{}", t.timecode(Some(r60)));
        }
        assert_eq!(Flicks::parse("1:02:03:30", Some(r60)), Some(Flicks::from_seconds(3723.5)));
        assert_eq!(Flicks::parse("1:02:03.5", None), Some(Flicks::from_seconds(3723.5)), "tenths");
        assert_eq!(Flicks::parse("2:03.250", None), Some(Flicks::from_millis(123_250)));
        assert_eq!(Flicks::parse("45", None), Some(Flicks::from_seconds(45.0)));
        assert_eq!(Flicks::parse("45.5", Some(r60)), Some(Flicks::from_seconds(45.5)), "milliseconds win, with a rate");
        assert_eq!(Flicks::parse("-0:00:01.000", None), Some(Flicks(-PER_SECOND)));
        assert_eq!(Flicks::parse("1:02:03:30", None), None, "frames need a rate");
        assert_eq!(Flicks::parse("1:02:03:30:1", Some(r60)), None);
        assert_eq!(Flicks::parse("abc", None), None);
        assert_eq!(Flicks::parse("", None), None);
        assert_eq!(Flicks::parse("1:-2", None), None);
    }
}
