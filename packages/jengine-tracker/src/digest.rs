//! A rendered song's fingerprint, for checking this player against jSuite's.
//!
//! A port of jSuite's `src/lib/tracker/digest.ts`. The two players cannot
//! agree bit for bit - `sin`, `exp`, `pow` and `tanh` may differ in the
//! last place between libms - so the fixtures store, per window of
//! [`WINDOW`] frames, the RMS of each side and the zero crossings of the
//! left: level and pan catch envelope and gain mistakes, crossings catch
//! pitch, and the window index catches timing.

use serde::{Deserialize, Serialize};

pub const WINDOW: usize = 2048;
/// Crossings are counted with hysteresis so silence's float noise is not.
pub const HYSTERESIS: f64 = 0.01;
/// RMS is stored as an integer in units of 1e-5.
pub const SCALE: f64 = 100000.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Digest {
    pub rate: u32,
    pub window: usize,
    pub frames: u64,
    pub loop_start: Option<u64>,
    pub rms_l: Vec<i64>,
    pub rms_r: Vec<i64>,
    pub crossings: Vec<i64>,
}

/// JavaScript's `Math.round`, which `digest.ts` stores with.
fn js_round(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

pub fn digest(left: &[f32], right: &[f32], rate: u32, loop_start: Option<u64>) -> Digest {
    let mut out = Digest {
        rate,
        window: WINDOW,
        frames: left.len() as u64,
        loop_start,
        rms_l: Vec::new(),
        rms_r: Vec::new(),
        crossings: Vec::new(),
    };
    let mut sign = 0;
    for start in (0..left.len()).step_by(WINDOW) {
        let end = left.len().min(start + WINDOW);
        let (mut sl, mut sr, mut crossings) = (0.0f64, 0.0f64, 0);
        for i in start..end {
            let l = f64::from(left[i]);
            let r = f64::from(right[i]);
            sl += l * l;
            sr += r * r;
            let s = if l > HYSTERESIS {
                1
            } else if l < -HYSTERESIS {
                -1
            } else {
                0
            };
            if s != 0 {
                if sign != 0 && s != sign {
                    crossings += 1;
                }
                sign = s;
            }
        }
        let n = (end - start) as f64;
        out.rms_l.push(js_round((sl / n).sqrt() * SCALE));
        out.rms_r.push(js_round((sr / n).sqrt() * SCALE));
        out.crossings.push(crossings);
    }
    out
}

/// Every place two digests differ by more than the tolerances, as
/// `(window, field, expected, actual)`; window -1 is the whole song. Frame
/// counts and the loop point must match exactly - timing has no excuse.
pub fn compare(expected: &Digest, actual: &Digest, rms_tolerance: i64, crossing_tolerance: i64) -> Vec<(i64, &'static str, i64, i64)> {
    let mut out = Vec::new();
    if expected.frames != actual.frames {
        out.push((-1, "frames", expected.frames as i64, actual.frames as i64));
    }
    if expected.loop_start != actual.loop_start {
        let v = |l: Option<u64>| l.map_or(-1, |l| l as i64);
        out.push((-1, "loop_start", v(expected.loop_start), v(actual.loop_start)));
    }
    let n = expected.rms_l.len().min(actual.rms_l.len());
    for w in 0..n {
        for (field, e, a) in [("rms_l", &expected.rms_l, &actual.rms_l), ("rms_r", &expected.rms_r, &actual.rms_r)] {
            if (e[w] - a[w]).abs() > rms_tolerance {
                out.push((w as i64, field, e[w], a[w]));
            }
        }
        if (expected.crossings[w] - actual.crossings[w]).abs() > crossing_tolerance {
            out.push((w as i64, "crossings", expected.crossings[w], actual.crossings[w]));
        }
    }
    out
}
