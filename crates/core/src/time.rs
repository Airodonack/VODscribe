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
                // Round the whole moment, so 0.9996 s carries into the next second.
                let ms = div_round(i128::from(t.0), i128::from(Self::MILLISECOND.0)) as i64;
                let (whole, ms) = (ms / 1000, ms % 1000);
                format!("{sign}{}:{:02}:{:02}.{ms:03}", whole / 3600, whole / 60 % 60, whole % 60)
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
            if !is_digits(ms) {
                return None;
            }
            (clock, Some(format!("{ms:0<3}")[..3].parse::<i64>().ok()?), None)
        } else if text.matches(':').count() == 3 {
            let (clock, ff) = text.rsplit_once(':')?;
            (clock, None, Some(if is_digits(ff) { ff.parse::<i64>().ok()? } else { return None }))
        } else {
            (text, None, None)
        };
        let parts: Vec<i64> =
            clock.split(':').map(|p| if is_digits(p) { p.parse().ok() } else { None }).collect::<Option<_>>()?;
        if parts.is_empty() || parts.len() > 3 {
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

/// Digits only: no sign, no space.
fn is_digits(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
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
        let rate =
            near(fps, 1).or_else(|| near(fps, 1001).filter(|r| r.num % 1000 == 0)).or_else(|| near(fps, 1000))?;
        let (mut a, mut b) = (rate.num, rate.den);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Some(Rate::new(rate.num / a, rate.den / a))
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
mod tests;
